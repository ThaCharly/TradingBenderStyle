//! Estado completo del núcleo, checkpoints y manifiestos de corrida (doc 02 §7, doc 05
//! §10).
//!
//! - [`KernelState`] junta las proyecciones del núcleo (reloj, velas por fuente,
//!   compuestas, objetos del usuario y cobertura) y aplica los `Envelope` en orden K.
//!   Sus salidas (velas cerradas y revisadas) se encadenan en un hash de salida.
//! - **Checkpoint**: los bytes canónicos del estado más su hash BLAKE3. Al leerlo se
//!   verifica el hash, así un checkpoint corrupto se detecta siempre.
//!   [`restore`] vuelve al estado en `T` desde el checkpoint diario más cercano y, si está
//!   corrupto, desde el anterior.
//! - [`RunManifest`]: segmentos leídos, commit, hash de configuración y hash de salida.

use std::collections::BTreeMap;

use bender_types::NS_PER_DAY;
use bender_types::proto::{self, envelope, market_event};

use crate::bars::{BarAggregator, BarConfig, BarError, BarEvent, BarEventKind, Timeframe};
use crate::canonical::{self, CheckpointError, Checkpointable, Decoder, Encoder};
use crate::clock::{KernelClock, OutOfOrder};
use crate::composite::{CompositeAggregator, CompositeConfig, CompositeDefinition, CompositeEvent};
use crate::coverage::CoverageProjection;
use crate::order::{SourceRankTable, UnknownSource};
use crate::user::{KnownVersions, UserProjection};
use crate::versioned::StaleUpdate;

/// Demoras de disponibilidad para datos reconstruidos (doc 05 §4).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AvailabilityDelays {
    /// δ_trade: trade de dump público. Default 500 ms.
    pub trade_ns: i64,
    /// δ_bar: vela de terceros. Default 2 s.
    pub bar_ns: i64,
    /// δ_stat: funding/OI histórico. Default 5 s.
    pub stat_ns: i64,
}

impl Default for AvailabilityDelays {
    fn default() -> Self {
        Self {
            trade_ns: 500_000_000,
            bar_ns: 2_000_000_000,
            stat_ns: 5_000_000_000,
        }
    }
}

/// Configuración versionada de una corrida del núcleo. Su hash va al manifiesto: cambiar
/// cualquier parámetro cambia el hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KernelConfig {
    pub timeframes: Vec<Timeframe>,
    /// `L` y horizonte de revisión de las velas.
    pub bars: BarConfig,
    /// `L`, `M` y horizonte de las compuestas.
    pub composite: CompositeConfig,
    pub composites: Vec<CompositeDefinition>,
    /// `G`: grilla de latidos.
    pub heartbeat_grid_ns: i64,
    pub delays: AvailabilityDelays,
    pub source_rank_version: u32,
    pub source_rank_hash: String,
    pub known_versions: KnownVersions,
}

impl Default for KernelConfig {
    fn default() -> Self {
        let table = SourceRankTable::current();
        Self {
            timeframes: Timeframe::STANDARD.to_vec(),
            bars: BarConfig::default(),
            composite: CompositeConfig::default(),
            composites: Vec::new(),
            heartbeat_grid_ns: crate::heartbeat::DEFAULT_GRID_NS,
            delays: AvailabilityDelays::default(),
            source_rank_version: table.version(),
            source_rank_hash: table.content_hash().to_string(),
            known_versions: KnownVersions::default(),
        }
    }
}

impl KernelConfig {
    /// BLAKE3 de la configuración canónica.
    pub fn hash(&self) -> String {
        canonical::hash(self)
    }
}

/// Error al aplicar un evento.
#[derive(Debug, PartialEq, Eq)]
pub enum KernelError {
    OutOfOrder(OutOfOrder),
    UnknownSource(UnknownSource),
    Bar(BarError),
    Stale(StaleUpdate),
    MissingSource,
}

impl std::fmt::Display for KernelError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::OutOfOrder(e) => write!(f, "{e}"),
            Self::UnknownSource(e) => write!(f, "{e}"),
            Self::Bar(e) => write!(f, "{e}"),
            Self::Stale(e) => write!(f, "{e}"),
            Self::MissingSource => write!(f, "evento sin fuente"),
        }
    }
}

impl std::error::Error for KernelError {}

/// Salida del núcleo.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KernelOutput {
    Bar(BarEvent),
    Composite(CompositeEvent),
}

/// Estado completo del núcleo.
#[derive(Clone, Debug)]
pub struct KernelState {
    config: KernelConfig,
    clock: KernelClock<()>,
    bars: BTreeMap<String, BarAggregator>,
    composites: Vec<CompositeAggregator>,
    user: UserProjection,
    coverage: CoverageProjection,
    events: u64,
    /// Hash encadenado de todas las salidas: `h ← BLAKE3(h ‖ salida canónica)`.
    output_hash: [u8; 32],
}

impl KernelState {
    pub fn new(config: KernelConfig) -> Self {
        let composites = config
            .composites
            .iter()
            .map(|d| CompositeAggregator::new(d.clone(), &config.timeframes, config.composite))
            .collect();
        Self {
            user: UserProjection::new(config.known_versions),
            config,
            clock: KernelClock::new(),
            bars: BTreeMap::new(),
            composites,
            coverage: CoverageProjection::new(),
            events: 0,
            output_hash: [0; 32],
        }
    }

    pub fn config(&self) -> &KernelConfig {
        &self.config
    }

    /// `ts_available` del último evento aplicado.
    pub fn now(&self) -> Option<i64> {
        self.clock.now()
    }

    pub fn events_applied(&self) -> u64 {
        self.events
    }

    pub fn bars(&self, source: &str) -> Option<&BarAggregator> {
        self.bars.get(source)
    }

    pub fn composites(&self) -> &[CompositeAggregator] {
        &self.composites
    }

    pub fn user(&self) -> &UserProjection {
        &self.user
    }

    pub fn coverage(&self) -> &CoverageProjection {
        &self.coverage
    }

    /// Hash encadenado de las salidas emitidas hasta ahora.
    pub fn output_hash(&self) -> String {
        hex(&self.output_hash)
    }

    /// Hash del estado completo (bytes canónicos).
    pub fn state_hash(&self) -> String {
        canonical::hash(self)
    }

    /// Aplica un evento en orden K y agrega sus salidas a `out`.
    pub fn apply(
        &mut self,
        env: &proto::Envelope,
        out: &mut Vec<KernelOutput>,
    ) -> Result<(), KernelError> {
        let source = env.source.as_ref().ok_or(KernelError::MissingSource)?;
        let order = SourceRankTable::current()
            .source_order(source)
            .map_err(KernelError::UnknownSource)?;
        let mut fired = Vec::new();
        self.clock
            .advance(env, &order, &mut fired)
            .map_err(KernelError::OutOfOrder)?;
        let first_new = out.len();
        match &env.payload {
            Some(envelope::Payload::Market(proto::MarketEvent {
                event: Some(market_event::Event::Trade(_)),
            })) => {
                let mut bar_events = Vec::new();
                let config = &self.config;
                self.bars
                    .entry(order.key.clone())
                    .or_insert_with(|| {
                        BarAggregator::new(&order.key, &config.timeframes, config.bars)
                    })
                    .on_envelope(env, &mut bar_events)
                    .map_err(KernelError::Bar)?;
                out.extend(bar_events.into_iter().map(KernelOutput::Bar));
                let tick = crate::bars::trade_tick(env).map_err(KernelError::Bar)?;
                for c in &mut self.composites {
                    let mut ev = Vec::new();
                    c.on_trade(&order.key, &tick, env.ts_available, &mut ev)
                        .map_err(KernelError::Bar)?;
                    out.extend(ev.into_iter().map(KernelOutput::Composite));
                }
            }
            Some(envelope::Payload::Heartbeat(_)) => {
                for agg in self.bars.values_mut() {
                    let mut ev = Vec::new();
                    agg.on_heartbeat(env.ts_available, &mut ev);
                    out.extend(ev.into_iter().map(KernelOutput::Bar));
                }
                for c in &mut self.composites {
                    let mut ev = Vec::new();
                    c.on_heartbeat(env.ts_available, &mut ev);
                    out.extend(ev.into_iter().map(KernelOutput::Composite));
                }
            }
            Some(envelope::Payload::Gap(_)) | Some(envelope::Payload::GapRecovery(_)) => {
                self.coverage.apply(env);
            }
            Some(envelope::Payload::User(_)) => {
                self.user.apply(env).map_err(KernelError::Stale)?;
            }
            _ => {}
        }
        for o in &out[first_new..] {
            let mut h = blake3::Hasher::new();
            h.update(&self.output_hash);
            h.update(&canonical::to_bytes(o));
            self.output_hash = *h.finalize().as_bytes();
        }
        self.events += 1;
        Ok(())
    }

    /// Checkpoint: bytes canónicos del estado seguidos de su BLAKE3 (32 bytes).
    pub fn checkpoint(&self) -> Vec<u8> {
        let mut bytes = canonical::to_bytes(self);
        let hash = blake3::hash(&bytes);
        bytes.extend_from_slice(hash.as_bytes());
        bytes
    }

    /// Reconstruye el estado de un checkpoint, verificando su hash.
    pub fn from_checkpoint(bytes: &[u8]) -> Result<Self, CheckpointError> {
        if bytes.len() < 32 {
            return Err(CheckpointError::Truncated);
        }
        let (body, stored) = bytes.split_at(bytes.len() - 32);
        let actual = blake3::hash(body);
        if actual.as_bytes() != stored {
            return Err(CheckpointError::Corrupt {
                expected: hex(stored),
                actual: actual.to_hex().to_string(),
            });
        }
        canonical::from_bytes(body)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// Inicio (00:00 UTC) del día de checkpoint que contiene `ts`.
pub fn checkpoint_day_start(ts: i64) -> i64 {
    ts.div_euclid(NS_PER_DAY) * NS_PER_DAY
}

/// De dónde salen los checkpoints guardados (en producción, `bender-store`).
pub trait CheckpointSource {
    /// Bytes del checkpoint tomado al inicio del día `day_start` (estado con todos los
    /// eventos de `ts_available < day_start`), si existe.
    fn checkpoint(&self, day_start: i64) -> Option<Vec<u8>>;
}

/// Cómo se llegó a un estado restaurado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Restored {
    pub state: KernelState,
    /// Día del checkpoint usado (`None` = desde el inicio).
    pub from_checkpoint: Option<i64>,
    /// Checkpoints descartados por corruptos.
    pub corrupt: Vec<i64>,
}

/// Estado en `t` (todos los eventos con `ts_available ≤ t`): desde el checkpoint diario
/// más cercano anterior a `t`. Si un checkpoint está corrupto se descarta y se usa el
/// anterior; si no hay ninguno sano, desde el inicio. `events(desde, hasta)` devuelve los
/// eventos con `desde ≤ ts_available ≤ hasta` en orden K (`desde = None`: desde el
/// principio). Se busca hacia atrás a lo sumo `max_days` días.
pub fn restore(
    config: &KernelConfig,
    t: i64,
    checkpoints: &dyn CheckpointSource,
    events: &dyn Fn(Option<i64>, i64) -> Vec<proto::Envelope>,
    max_days: u32,
) -> Result<Restored, KernelError> {
    let mut corrupt = Vec::new();
    let mut day = checkpoint_day_start(t);
    for _ in 0..max_days {
        if let Some(bytes) = checkpoints.checkpoint(day) {
            match KernelState::from_checkpoint(&bytes) {
                Ok(mut state) if state.config == *config => {
                    let mut out = Vec::new();
                    for e in events(Some(day), t) {
                        state.apply(&e, &mut out)?;
                    }
                    return Ok(Restored {
                        state,
                        from_checkpoint: Some(day),
                        corrupt,
                    });
                }
                _ => corrupt.push(day),
            }
        }
        day -= NS_PER_DAY;
    }
    let mut state = KernelState::new(config.clone());
    let mut out = Vec::new();
    for e in events(None, t) {
        state.apply(&e, &mut out)?;
    }
    Ok(Restored {
        state,
        from_checkpoint: None,
        corrupt,
    })
}

impl PartialEq for KernelState {
    fn eq(&self, other: &Self) -> bool {
        canonical::to_bytes(self) == canonical::to_bytes(other)
    }
}

impl Eq for KernelState {}

/// Manifiesto de una corrida (doc 02 §7): con esto cualquier resultado se puede
/// recalcular y verificar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunManifest {
    /// Segmentos leídos: `(ruta, hash de contenido)`, en orden de ruta.
    pub segments: BTreeMap<String, String>,
    pub git_commit: String,
    pub config_hash: String,
    pub output_hash: String,
}

impl RunManifest {
    pub fn new(
        segments: BTreeMap<String, String>,
        git_commit: &str,
        config: &KernelConfig,
        state: &KernelState,
    ) -> Self {
        Self {
            segments,
            git_commit: git_commit.to_string(),
            config_hash: config.hash(),
            output_hash: state.output_hash(),
        }
    }

    pub fn hash(&self) -> String {
        canonical::hash(self)
    }
}

// ---------------------------------------------------------------------------------------
// Serialización canónica.

crate::checkpoint_struct!(AvailabilityDelays {
    trade_ns,
    bar_ns,
    stat_ns
});
crate::checkpoint_struct!(KernelConfig {
    timeframes,
    bars,
    composite,
    composites,
    heartbeat_grid_ns,
    delays,
    source_rank_version,
    source_rank_hash,
    known_versions,
});
crate::checkpoint_struct!(RunManifest {
    segments,
    git_commit,
    config_hash,
    output_hash
});

impl Checkpointable for BarEventKind {
    fn encode(&self, e: &mut Encoder) {
        e.put(&u8::from(*self == Self::Revised));
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(if d.get::<bool>()? {
            Self::Revised
        } else {
            Self::Closed
        })
    }
}

impl Checkpointable for KernelOutput {
    fn encode(&self, e: &mut Encoder) {
        match self {
            Self::Bar(b) => {
                e.put(&0u8)
                    .put(&b.kind)
                    .put(&b.source.to_string())
                    .put(&b.bar)
                    .put(&b.ts_available);
            }
            Self::Composite(c) => {
                e.put(&1u8).put(&c.kind).put(&c.bar).put(&c.ts_available);
            }
        }
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        match d.get::<u8>()? {
            0 => Ok(Self::Bar(BarEvent {
                kind: d.get()?,
                source: d.get::<String>()?.into(),
                bar: d.get()?,
                ts_available: d.get()?,
            })),
            1 => Ok(Self::Composite(CompositeEvent {
                kind: d.get()?,
                bar: d.get()?,
                ts_available: d.get()?,
            })),
            x => Err(CheckpointError::Invalid(format!("KernelOutput {x}"))),
        }
    }
}

impl Checkpointable for KernelState {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.config)
            .put(&self.clock)
            .put(&self.bars)
            .put(&self.composites)
            .put(&self.user)
            .put(&self.coverage)
            .put(&self.events)
            .bytes(&self.output_hash);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        let config = d.get()?;
        let clock = d.get()?;
        let bars = d.get()?;
        let composites = d.get()?;
        let user = d.get()?;
        let coverage = d.get()?;
        let events = d.get()?;
        let output_hash: [u8; 32] = d
            .bytes()?
            .try_into()
            .map_err(|_| CheckpointError::Invalid("output_hash".into()))?;
        Ok(Self {
            config,
            clock,
            bars,
            composites,
            user,
            coverage,
            events,
            output_hash,
        })
    }
}
