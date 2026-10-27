//! Velas (doc 05 §5–6): agregación por fuente de trades en velas de 1 s a 1 año, con
//! cierre por marca de agua o por latido, datos tardíos y revisiones.
//!
//! - **Alineación** (UTC): las temporalidades de segundos (1 s … 1 D, 3 D y las
//!   personalizadas) se alinean al epoch; **3 D también** (bloques de 3 días desde
//!   1970-01-01, no desde un lunes ni desde el día 1 del mes). La semana empieza el lunes
//!   00:00. Mes, trimestre y año son calendario.
//! - **Cierre**: una vela `[t, t+d)` cierra cuando `t+d ≤ hasta_dónde_se_sabe`, que es el
//!   máximo entre la marca de agua (`max ts_event − L`) y el último latido menos `L`. O sea:
//!   la marca de agua alcanza `t+d`, o llega un latido con `ts_available ≥ t+d+L`.
//! - **Tardíos**: un trade cuya vela ya cerró se aplica igual y emite una revisión (n+1).
//!   La revisión 1 queda guardada. Las velas cerradas se conservan durante
//!   `revision_horizon` para poder revisarlas; un trade más viejo que eso se cuenta en
//!   [`BarAggregator::late_dropped`] y no cambia ninguna vela.
//! - Se agrega **siempre desde los trades** para todas las temporalidades. No hay
//!   derivación jerárquica (4h desde 1m), así que no hay diferencias posibles entre ambas.

use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use bender_types::proto::{self, envelope, market_event};
use bender_types::{Decimal, DecimalError, NS_PER_DAY, civil_from_days, days_from_civil};

const NS_PER_SECOND: i64 = 1_000_000_000;

/// Temporalidad de una vela.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Timeframe {
    /// N segundos, alineado al epoch (incluye 1 D y 3 D).
    Seconds(u32),
    /// Semana desde el lunes 00:00 UTC.
    Week,
    /// N meses calendario (1 = mes, 3 = trimestre). N tiene que dividir a 12.
    Months(u32),
    /// Año calendario.
    Year,
}

impl Timeframe {
    /// Las 19 temporalidades estándar, de menor a mayor.
    pub const STANDARD: [Timeframe; 19] = [
        Self::Seconds(1),
        Self::Seconds(5),
        Self::Seconds(15),
        Self::Seconds(60),
        Self::Seconds(180),
        Self::Seconds(300),
        Self::Seconds(900),
        Self::Seconds(1_800),
        Self::Seconds(3_600),
        Self::Seconds(7_200),
        Self::Seconds(14_400),
        Self::Seconds(21_600),
        Self::Seconds(43_200),
        Self::Seconds(86_400),
        Self::Seconds(259_200),
        Self::Week,
        Self::Months(1),
        Self::Months(3),
        Self::Year,
    ];

    /// Temporalidad personalizada de `seconds` segundos (alineada al epoch).
    pub fn custom_seconds(seconds: u32) -> Option<Self> {
        (seconds > 0).then_some(Self::Seconds(seconds))
    }

    /// Inicio de la vela que contiene `ts` (ns UTC).
    pub fn bucket_start(self, ts: i64) -> i64 {
        match self {
            Self::Seconds(n) => {
                let step = i64::from(n) * NS_PER_SECOND;
                ts.div_euclid(step) * step
            }
            Self::Week => {
                // 1970-01-05 (día 4) fue lunes.
                let days = ts.div_euclid(NS_PER_DAY);
                ((days - 4).div_euclid(7) * 7 + 4) * NS_PER_DAY
            }
            Self::Months(n) => {
                let (year, month, _) = civil_from_days(ts.div_euclid(NS_PER_DAY));
                let first = (month - 1) / n * n + 1;
                days_from_civil(year, first, 1) * NS_PER_DAY
            }
            Self::Year => {
                let (year, _, _) = civil_from_days(ts.div_euclid(NS_PER_DAY));
                days_from_civil(year, 1, 1) * NS_PER_DAY
            }
        }
    }

    /// Fin (exclusivo) de la vela que empieza en `start`.
    pub fn bucket_end(self, start: i64) -> i64 {
        match self {
            Self::Seconds(n) => start + i64::from(n) * NS_PER_SECOND,
            Self::Week => start + 7 * NS_PER_DAY,
            Self::Months(n) => {
                let (year, month, _) = civil_from_days(start.div_euclid(NS_PER_DAY));
                let next = month + n;
                let (y, m) = if next > 12 {
                    (year + 1, next - 12)
                } else {
                    (year, next)
                };
                days_from_civil(y, m, 1) * NS_PER_DAY
            }
            Self::Year => {
                let (year, _, _) = civil_from_days(start.div_euclid(NS_PER_DAY));
                days_from_civil(year + 1, 1, 1) * NS_PER_DAY
            }
        }
    }
}

impl fmt::Display for Timeframe {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::Seconds(n) if n % 86_400 == 0 => write!(f, "{}D", n / 86_400),
            Self::Seconds(n) if n % 3_600 == 0 => write!(f, "{}h", n / 3_600),
            Self::Seconds(n) if n % 60 == 0 => write!(f, "{}m", n / 60),
            Self::Seconds(n) => write!(f, "{n}s"),
            Self::Week => write!(f, "1W"),
            Self::Months(n) => write!(f, "{n}M"),
            Self::Year => write!(f, "1Y"),
        }
    }
}

/// Estado de una vela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarState {
    Forming,
    Closed,
}

/// Una vela (doc 05 §6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bar {
    pub timeframe: Timeframe,
    /// `[start, end)`, ns UTC.
    pub start: i64,
    pub end: i64,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume_base: Decimal,
    /// Σ precio × cantidad.
    pub volume_quote: Decimal,
    /// Volumen con agresor comprador / vendedor. Los trades sin agresor informado sólo
    /// cuentan en `volume_base`.
    pub buy_volume_base: Decimal,
    pub sell_volume_base: Decimal,
    pub trade_count: u64,
    pub first_ts_event: i64,
    pub last_ts_event: i64,
    /// 1 para la primera versión; cada revisión por tardíos suma 1.
    pub revision: u32,
    pub state: BarState,
}

/// Un trade, ya extraído del `Envelope`.
#[derive(Clone, Copy, Debug)]
pub struct TradeTick {
    pub ts_event: i64,
    pub price: Decimal,
    pub qty_base: Decimal,
    pub aggressor: proto::Side,
}

impl Bar {
    fn new(
        timeframe: Timeframe,
        start: i64,
        trade: &TradeTick,
        notional: Decimal,
    ) -> Result<Self, BarError> {
        let mut bar = Self {
            timeframe,
            start,
            end: timeframe.bucket_end(start),
            open: trade.price,
            high: trade.price,
            low: trade.price,
            close: trade.price,
            volume_base: Decimal::ZERO,
            volume_quote: Decimal::ZERO,
            buy_volume_base: Decimal::ZERO,
            sell_volume_base: Decimal::ZERO,
            trade_count: 0,
            first_ts_event: trade.ts_event,
            last_ts_event: trade.ts_event,
            revision: 1,
            state: BarState::Forming,
        };
        bar.add_volume(trade, notional)?;
        Ok(bar)
    }

    fn add_volume(&mut self, t: &TradeTick, notional: Decimal) -> Result<(), BarError> {
        self.volume_base = self.volume_base.checked_add(t.qty_base)?;
        self.volume_quote = self.volume_quote.checked_add(notional)?;
        match t.aggressor {
            proto::Side::Buy => {
                self.buy_volume_base = self.buy_volume_base.checked_add(t.qty_base)?
            }
            proto::Side::Sell => {
                self.sell_volume_base = self.sell_volume_base.checked_add(t.qty_base)?
            }
            proto::Side::Unspecified => {}
        }
        self.trade_count += 1;
        Ok(())
    }

    /// Aplica un trade. Apertura = trade de menor `ts_event` (el primero en llegar si
    /// empatan); cierre = el de mayor `ts_event` (el último en llegar si empatan).
    fn apply(&mut self, t: &TradeTick, notional: Decimal) -> Result<(), BarError> {
        if t.ts_event < self.first_ts_event {
            self.first_ts_event = t.ts_event;
            self.open = t.price;
        }
        if t.ts_event >= self.last_ts_event {
            self.last_ts_event = t.ts_event;
            self.close = t.price;
        }
        if t.price > self.high {
            self.high = t.price;
        }
        if t.price < self.low {
            self.low = t.price;
        }
        self.add_volume(t, notional)
    }
}

/// Qué pasó con una vela.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BarEventKind {
    /// Primera versión cerrada (revisión 1).
    Closed,
    /// Revisión de una vela ya cerrada por un trade tardío.
    Revised,
}

/// Evento de salida del agregador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BarEvent {
    pub kind: BarEventKind,
    /// Clave canónica de la fuente.
    pub source: Arc<str>,
    pub bar: Bar,
    /// `ts_available` del evento que provocó el cierre o la revisión: es cuándo se supo.
    pub ts_available: i64,
}

/// Error del agregador.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BarError {
    /// Trade sin precio o sin cantidad.
    InvalidTrade,
    Decimal(DecimalError),
}

impl From<DecimalError> for BarError {
    fn from(e: DecimalError) -> Self {
        Self::Decimal(e)
    }
}

impl fmt::Display for BarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidTrade => write!(f, "trade sin precio o sin cantidad"),
            Self::Decimal(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for BarError {}

/// Parámetros del agregador (parte de la configuración versionada de la corrida).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BarConfig {
    /// Demora permitida `L` (doc 05 §5). Default 2 s.
    pub lateness_ns: i64,
    /// Cuánto tiempo se conservan las velas cerradas para poder revisarlas. Default 24 h.
    pub revision_horizon_ns: i64,
}

impl Default for BarConfig {
    fn default() -> Self {
        Self {
            lateness_ns: 2 * NS_PER_SECOND,
            revision_horizon_ns: NS_PER_DAY,
        }
    }
}

#[derive(Clone, Debug)]
struct TfState {
    tf: Timeframe,
    /// Última vela calculada `[inicio, fin)`: la mayoría de los trades caen ahí y se evita
    /// recalcular el calendario.
    last_bucket: (i64, i64),
    /// Velas abiertas por inicio.
    forming: BTreeMap<i64, Bar>,
    /// Velas cerradas por inicio, con todas sus revisiones (la última es la vigente).
    closed: BTreeMap<i64, Vec<Bar>>,
}

/// Agregador de velas de una fuente de trades.
#[derive(Clone, Debug)]
pub struct BarAggregator {
    source: Arc<str>,
    config: BarConfig,
    tfs: Vec<TfState>,
    max_ts_event: Option<i64>,
    last_heartbeat: Option<i64>,
    late_dropped: u64,
    /// Menor fin entre las velas en formación: antes de eso no hay nada para cerrar.
    next_close: i64,
}

impl BarAggregator {
    pub fn new(source: &str, timeframes: &[Timeframe], config: BarConfig) -> Self {
        Self {
            source: source.into(),
            config,
            tfs: timeframes
                .iter()
                .map(|&tf| TfState {
                    tf,
                    last_bucket: (0, 0),
                    forming: BTreeMap::new(),
                    closed: BTreeMap::new(),
                })
                .collect(),
            max_ts_event: None,
            last_heartbeat: None,
            late_dropped: 0,
            next_close: i64::MAX,
        }
    }

    /// Hasta dónde se sabe que no llegan más trades en orden: el máximo entre la marca de
    /// agua (`max ts_event − L`) y el último latido menos `L`.
    pub fn known_until(&self) -> Option<i64> {
        let l = self.config.lateness_ns;
        let wm = self.max_ts_event.map(|t| t - l);
        let hb = self.last_heartbeat.map(|t| t - l);
        wm.max(hb)
    }

    /// Trades descartados por llegar después del horizonte de revisión.
    pub fn late_dropped(&self) -> u64 {
        self.late_dropped
    }

    /// Velas en formación de una temporalidad, por inicio.
    pub fn forming(&self, tf: Timeframe) -> impl Iterator<Item = &Bar> {
        self.tfs
            .iter()
            .filter(move |s| s.tf == tf)
            .flat_map(|s| s.forming.values())
    }

    /// Todas las versiones conservadas de una vela cerrada (revisión 1 primero).
    pub fn revisions(&self, tf: Timeframe, start: i64) -> Option<&[Bar]> {
        self.tfs
            .iter()
            .find(|s| s.tf == tf)
            .and_then(|s| s.closed.get(&start))
            .map(Vec::as_slice)
    }

    /// Procesa un `Envelope` en orden K: los trades de esta fuente se agregan, los latidos
    /// (de cualquier fuente) pueden cerrar velas, y el resto se ignora.
    pub fn on_envelope(
        &mut self,
        env: &proto::Envelope,
        out: &mut Vec<BarEvent>,
    ) -> Result<(), BarError> {
        match &env.payload {
            Some(envelope::Payload::Market(proto::MarketEvent {
                event: Some(market_event::Event::Trade(t)),
            })) => {
                let tick = TradeTick {
                    ts_event: env.ts_event,
                    price: t.price.ok_or(BarError::InvalidTrade)?.try_into()?,
                    qty_base: t.qty_base.ok_or(BarError::InvalidTrade)?.try_into()?,
                    aggressor: proto::Side::try_from(t.aggressor_side)
                        .unwrap_or(proto::Side::Unspecified),
                };
                self.on_trade(&tick, env.ts_available, out)
            }
            Some(envelope::Payload::Heartbeat(_)) => {
                self.on_heartbeat(env.ts_available, out);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Agrega un trade que se conoció en `ts_available`.
    pub fn on_trade(
        &mut self,
        trade: &TradeTick,
        ts_available: i64,
        out: &mut Vec<BarEvent>,
    ) -> Result<(), BarError> {
        let known_before = self.known_until();
        let horizon = self.config.revision_horizon_ns;
        let mut dropped = false;
        let notional = trade.price.checked_mul(trade.qty_base)?;
        for state in &mut self.tfs {
            let (start, end) =
                if (state.last_bucket.0..state.last_bucket.1).contains(&trade.ts_event) {
                    state.last_bucket
                } else {
                    let start = state.tf.bucket_start(trade.ts_event);
                    state.last_bucket = (start, state.tf.bucket_end(start));
                    state.last_bucket
                };
            if let Some(bar) = state.forming.get_mut(&start) {
                bar.apply(trade, notional)?;
                continue;
            }
            if let Some(history) = state.closed.get_mut(&start) {
                let mut revised = history.last().expect("al menos una versión").clone();
                revised.apply(trade, notional)?;
                revised.revision += 1;
                history.push(revised.clone());
                out.push(BarEvent {
                    kind: BarEventKind::Revised,
                    source: self.source.clone(),
                    bar: revised,
                    ts_available,
                });
                continue;
            }
            match known_before {
                // Vela que ya habría cerrado.
                Some(known) if end <= known => {
                    if end.saturating_add(horizon) < known {
                        dropped = true;
                        continue;
                    }
                    let mut bar = Bar::new(state.tf, start, trade, notional)?;
                    bar.state = BarState::Closed;
                    state.closed.insert(start, vec![bar.clone()]);
                    out.push(BarEvent {
                        kind: BarEventKind::Closed,
                        source: self.source.clone(),
                        bar,
                        ts_available,
                    });
                }
                _ => {
                    state
                        .forming
                        .insert(start, Bar::new(state.tf, start, trade, notional)?);
                    self.next_close = self.next_close.min(end);
                }
            }
        }
        if dropped {
            self.late_dropped += 1;
        }
        self.max_ts_event = Some(
            self.max_ts_event
                .map_or(trade.ts_event, |m| m.max(trade.ts_event)),
        );
        self.close_ready(ts_available, out);
        Ok(())
    }

    /// Procesa un latido: cierra las velas con `fin + L ≤ ts_available`.
    pub fn on_heartbeat(&mut self, ts_available: i64, out: &mut Vec<BarEvent>) {
        self.last_heartbeat = Some(
            self.last_heartbeat
                .map_or(ts_available, |h| h.max(ts_available)),
        );
        self.close_ready(ts_available, out);
    }

    fn close_ready(&mut self, ts_available: i64, out: &mut Vec<BarEvent>) {
        let Some(known) = self.known_until() else {
            return;
        };
        if known < self.next_close {
            return;
        }
        let horizon = self.config.revision_horizon_ns;
        let mut next_close = i64::MAX;
        for state in &mut self.tfs {
            while let Some(entry) = state.forming.first_entry() {
                if entry.get().end > known {
                    break;
                }
                let (start, mut bar) = entry.remove_entry();
                bar.state = BarState::Closed;
                out.push(BarEvent {
                    kind: BarEventKind::Closed,
                    source: self.source.clone(),
                    bar: bar.clone(),
                    ts_available,
                });
                state.closed.insert(start, vec![bar]);
            }
            while let Some(entry) = state.closed.first_entry() {
                let end = entry.get().last().map_or(i64::MIN, |b| b.end);
                if end.saturating_add(horizon) >= known {
                    break;
                }
                entry.remove();
            }
            if let Some(bar) = state.forming.values().next() {
                next_close = next_close.min(bar.end);
            }
        }
        self.next_close = next_close;
    }
}
