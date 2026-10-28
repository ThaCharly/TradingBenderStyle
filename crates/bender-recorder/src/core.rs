//! Núcleo del grabador, **sin E/S**: recibe entradas con su instante (mensajes del WS,
//! respuestas REST, conexiones, ticks del reloj) y devuelve registros para el store y
//! pedidos para la capa de E/S. Así todo el manejo de huecos y recuperación se prueba con
//! tiempo simulado.
//!
//! Reglas (ADR-0011, `docs/venues/binance.md`):
//!
//! - **Huecos**: un hueco se emite cuando la fuente vuelve, que es cuando se conocen sus
//!   dos bordes (`to_ts` = primer dato visto tras reconectar, como dice el esquema):
//!   - desconexión o silencio: `Gap` de cada fuente de la conexión al reconectar, desde el
//!     último mensaje de la conexión;
//!   - salto de ids de trade: `Gap` al ver el trade que salta;
//!   - libro: `Gap` al resincronizar con un snapshot, desde el último delta válido;
//!   - reinicio: `Gap` (`PROCESS_DOWN`) desde el último latido grabado, para cada fuente
//!     que ya tenía datos.
//! - **Recuperación**: los trades perdidos se piden por REST desde el último id visto
//!   hasta el primero visto después del hueco; se graban con `provenance = recovered` y
//!   después un `GapRecovery` (`full` si la continuidad de ids se restablece). El libro se
//!   resincroniza (`RESNAPSHOT`, el intervalo queda `incomplete`). El resto de los datos no
//!   se recupera (`GapRecovery` con método `NONE`).
//! - Dentro de una fuente, `(ts_available, source_seq)` nunca retrocede: si dos entradas
//!   llegan con el mismo instante y la segunda tiene una secuencia menor, se le suma 1 ns.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use bender_store::{RawRecord, Store, StoreError, Zone};
use bender_types::proto::{
    self, GapReason, MarketDataKind, Provenance, RecoveryMethod, RecoveryResult, envelope,
    market_event,
};
use bender_types::{Ulid, source_key};
use bender_venue_binance::book::{DiffOutcome, LocalBook, SnapshotOutcome};
use bender_venue_binance::normalize::AggTrade;
use bender_venue_binance::{Market, Normalized, Parsed};

/// Cómo se graba el libro de un instrumento.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum BookMode {
    /// Snapshot REST + deltas del top-N.
    L2,
    /// Sólo el mejor precio de cada lado (`bookTicker`).
    Top,
}

/// Un instrumento a grabar.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Instrument {
    pub market: Market,
    pub symbol: String,
    pub book: BookMode,
}

/// Configuración del núcleo.
#[derive(Clone, Debug)]
pub struct CoreConfig {
    pub instruments: Vec<Instrument>,
    /// Niveles por lado del libro L2 (ADR-0009: 20).
    pub top_n: usize,
    /// Cada cuánto se graba un snapshot del top-N de un libro sincronizado.
    pub book_snapshot_every_ns: i64,
    /// Silencio de una conexión que se trata como caída.
    pub stale_after_ns: i64,
    /// Cada cuánto se pide el open interest (USDⓈ-M).
    pub open_interest_every_ns: i64,
    /// Trades por página de recuperación (máximo de Binance: 1000).
    pub recovery_page: u32,
    /// Páginas como máximo por recuperación; si no alcanza, queda `partial`.
    pub max_recovery_pages: u32,
}

impl CoreConfig {
    pub fn instruments(&self, market: Market) -> impl Iterator<Item = &Instrument> {
        self.instruments.iter().filter(move |i| i.market == market)
    }

    pub fn markets(&self) -> BTreeSet<Market> {
        self.instruments.iter().map(|i| i.market).collect()
    }
}

/// Entrada del núcleo. `ts` es el reloj de pared del grabador (ns UTC) al recibirla.
#[derive(Clone, Debug)]
pub enum Input {
    /// Se estableció la conexión WS de un mercado.
    Connected { market: Market, ts: i64 },
    /// Se cayó (o se cerró a pedido) la conexión WS de un mercado.
    Disconnected {
        market: Market,
        ts: i64,
        reason: GapReason,
        detail: String,
    },
    /// Mensaje del WS.
    Message {
        market: Market,
        ts: i64,
        raw: Vec<u8>,
    },
    /// Respuesta del snapshot REST del libro.
    Snapshot {
        market: Market,
        symbol: String,
        ts: i64,
        raw: Vec<u8>,
    },
    /// Página REST de `aggTrades` pedida desde `from_id`.
    AggTrades {
        market: Market,
        symbol: String,
        from_id: u64,
        ts: i64,
        raw: Vec<u8>,
    },
    /// La página de `aggTrades` no se pudo obtener (después de los reintentos).
    AggTradesFailed {
        market: Market,
        symbol: String,
        from_id: u64,
        ts: i64,
        detail: String,
    },
    /// Respuesta REST del open interest.
    OpenInterest {
        symbol: String,
        ts: i64,
        raw: Vec<u8>,
    },
    /// Tick del reloj (cada 100 ms).
    Tick { ts: i64 },
}

/// Pedido a la capa de E/S.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Snapshot REST del libro (prioridad alta).
    Snapshot { market: Market, symbol: String },
    /// Página de `aggTrades` desde un id (prioridad baja: recuperación).
    AggTrades {
        market: Market,
        symbol: String,
        from_id: u64,
        limit: u32,
    },
    /// Open interest (USDⓈ-M).
    OpenInterest { symbol: String },
    /// Cerrar y reabrir la conexión de un mercado.
    Reconnect {
        market: Market,
        reason: GapReason,
        detail: String,
    },
}

/// Registro para el store.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq)]
pub enum Record {
    Normalized {
        source: String,
        envelope: proto::Envelope,
    },
    Raw {
        source: String,
        record: RawRecord,
    },
}

/// Salida de una entrada.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Output {
    pub records: Vec<Record>,
    pub requests: Vec<Request>,
}

/// Lo que el núcleo necesita saber de corridas anteriores.
#[derive(Clone, Debug, Default)]
pub struct Startup {
    /// Último `ingest_seq` por zona y fuente.
    pub last_ingest: BTreeMap<(Zone, String), u64>,
    /// Último `ts_available` por fuente normalizada.
    pub last_ts: BTreeMap<String, i64>,
    /// Último id de trade agregado por fuente de trades.
    pub last_trade_id: BTreeMap<String, u64>,
}

impl Startup {
    /// Lee el estado de un store (que ya recuperó sus segmentos abiertos al abrirse).
    pub fn from_store(store: &Store) -> Result<Self, StoreError> {
        let mut s = Self::default();
        for seg in store.segments() {
            let last = s
                .last_ingest
                .entry((seg.zone, seg.source.clone()))
                .or_insert(0);
            *last = (*last).max(seg.ingest_seq_max);
            if seg.zone == Zone::Normalized {
                let ts = s.last_ts.entry(seg.source.clone()).or_insert(i64::MIN);
                *ts = (*ts).max(seg.ts_max);
            }
        }
        for (source, &ts) in &s.last_ts {
            if !source.ends_with(":trades") {
                continue;
            }
            let id = store
                .read_normalized(source, ts, i64::MAX)?
                .iter()
                .filter(|e| {
                    matches!(
                        &e.payload,
                        Some(envelope::Payload::Market(proto::MarketEvent {
                            event: Some(market_event::Event::Trade(_))
                        }))
                    )
                })
                .filter_map(|e| e.source_seq)
                .max();
            if let Some(id) = id {
                s.last_trade_id.insert(source.clone(), id);
            }
        }
        Ok(s)
    }

    /// Último instante en que el grabador estaba vivo: su último latido.
    pub fn last_alive(&self) -> Option<i64> {
        self.last_ts.get("CLOCK").copied()
    }
}

/// Nombre del mercado en las claves de fuente.
pub fn market_name(market: Market) -> &'static str {
    match market {
        Market::Spot => "SPOT",
        Market::UsdM => "PERP_LINEAR",
    }
}

/// Clave de la zona cruda: `BINANCE:<mercado>:<símbolo>:<ws|rest>:<stream>` (o sin
/// símbolo para lo que no se pudo interpretar).
pub fn raw_key(market: Market, symbol: Option<&str>, channel: &str) -> String {
    match symbol {
        Some(s) => format!("BINANCE:{}:{}:{channel}", market_name(market), s),
        None => format!("BINANCE:{}:{channel}", market_name(market)),
    }
}

fn ms(ms: i64) -> i64 {
    ms * 1_000_000
}

fn clock_source() -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Clock(proto::ClockSource {})),
    }
}

#[derive(Clone, Debug)]
struct Down {
    from: i64,
    reason: GapReason,
    detail: Option<String>,
}

#[derive(Clone, Debug, Default)]
struct Conn {
    up: bool,
    last_msg: Option<i64>,
    down: Option<Down>,
    reconnect_requested: bool,
}

#[derive(Clone, Copy, Debug)]
struct GapInfo {
    id: Ulid,
    from: i64,
    to: i64,
}

#[derive(Clone, Debug)]
struct Recovery {
    gaps: Vec<GapInfo>,
    start_id: u64,
    next_id: u64,
    /// Primer id visto en vivo después del hueco (no se pide).
    until_id: u64,
    pages: u32,
    written: u64,
    skipped: bool,
    /// Último `ts_event` del tramo continuo desde `start_id`.
    prefix_last_ts: Option<i64>,
}

#[derive(Clone, Debug, Default)]
struct TradeState {
    last_id: Option<u64>,
    last_ts: Option<i64>,
    /// Huecos de conexión que esperan el primer trade en vivo para recuperarse.
    awaiting: Vec<GapInfo>,
    recoveries: VecDeque<Recovery>,
}

#[derive(Clone, Debug)]
struct BookState {
    book: LocalBook,
    /// Hueco abierto: se emite al resincronizar.
    pending: Option<Down>,
    /// Último instante con el libro válido.
    last_ts: Option<i64>,
    requested: bool,
    /// Reintentos de snapshot seguidos que no engancharon, y cuándo pedir el próximo.
    retries: u32,
    retry_at: Option<i64>,
    next_periodic: i64,
}

/// Espera antes de pedir otro snapshot que no enganchó: 1 s, 2 s, 4 s… hasta 30 s.
fn snapshot_backoff(retries: u32) -> i64 {
    (1_000_000_000i64 << retries.min(5)).min(30_000_000_000)
}

/// El núcleo del grabador.
pub struct Core {
    cfg: CoreConfig,
    conns: BTreeMap<Market, Conn>,
    trades: BTreeMap<(Market, String), TradeState>,
    books: BTreeMap<(Market, String), BookState>,
    seq: BTreeMap<(Zone, String), u64>,
    /// Último `(ts_available, source_seq)` por fuente normalizada.
    stamps: BTreeMap<String, (i64, Option<u64>)>,
    /// Fuentes con datos de corridas anteriores.
    known: BTreeSet<String>,
    next_open_interest: i64,
}

impl Core {
    pub fn new(cfg: CoreConfig, startup: Startup) -> Self {
        let down = startup.last_alive().map(|from| Down {
            from,
            reason: GapReason::ProcessDown,
            detail: Some("el grabador estuvo detenido".into()),
        });
        let conns = cfg
            .markets()
            .into_iter()
            .map(|m| {
                (
                    m,
                    Conn {
                        down: down.clone(),
                        ..Conn::default()
                    },
                )
            })
            .collect();
        let mut trades = BTreeMap::new();
        let mut books = BTreeMap::new();
        for i in &cfg.instruments {
            let key = source_key(&bender_venue_binance::source(
                i.market,
                &i.symbol,
                MarketDataKind::Trades,
            ));
            trades.insert(
                (i.market, i.symbol.clone()),
                TradeState {
                    last_id: startup.last_trade_id.get(&key).copied(),
                    ..TradeState::default()
                },
            );
            if i.book == BookMode::L2 {
                books.insert(
                    (i.market, i.symbol.clone()),
                    BookState {
                        book: LocalBook::new(i.market, cfg.top_n),
                        pending: None,
                        last_ts: None,
                        requested: false,
                        retries: 0,
                        retry_at: None,
                        next_periodic: i64::MIN,
                    },
                );
            }
        }
        let stamps = startup
            .last_ts
            .iter()
            .map(|(k, &ts)| (k.clone(), (ts, None)))
            .collect();
        Self {
            known: startup.last_ts.keys().cloned().collect(),
            seq: startup.last_ingest,
            stamps,
            conns,
            trades,
            books,
            cfg,
            next_open_interest: i64::MIN,
        }
    }

    pub fn config(&self) -> &CoreConfig {
        &self.cfg
    }

    /// Procesa una entrada.
    pub fn handle(&mut self, input: Input) -> Output {
        let mut out = Output::default();
        match input {
            Input::Connected { market, ts } => self.on_connected(market, ts, &mut out),
            Input::Disconnected {
                market,
                ts,
                reason,
                detail,
            } => self.on_disconnected(market, ts, reason, detail),
            Input::Message { market, ts, raw } => self.on_message(market, ts, raw, &mut out),
            Input::Snapshot {
                market,
                symbol,
                ts,
                raw,
            } => self.on_snapshot(market, &symbol, ts, raw, &mut out),
            Input::AggTrades {
                market,
                symbol,
                from_id,
                ts,
                raw,
            } => self.on_agg_trades(market, &symbol, from_id, ts, Ok(raw), &mut out),
            Input::AggTradesFailed {
                market,
                symbol,
                from_id,
                ts,
                detail,
            } => self.on_agg_trades(market, &symbol, from_id, ts, Err(detail), &mut out),
            Input::OpenInterest { symbol, ts, raw } => {
                self.raw(
                    &mut out,
                    raw_key(Market::UsdM, Some(&symbol), "rest:openInterest"),
                    ts,
                    &raw,
                );
                if let Ok(n) = bender_venue_binance::open_interest(&raw) {
                    self.emit_normalized(&mut out, n, ts, Provenance::Live);
                }
            }
            Input::Tick { ts } => self.on_tick(ts, &mut out),
        }
        out
    }

    fn next_seq(&mut self, zone: Zone, source: &str) -> u64 {
        let s = self.seq.entry((zone, source.to_string())).or_insert(0);
        *s += 1;
        *s
    }

    /// `ts_available` para un evento de `source`: no retrocede dentro de la fuente.
    fn stamp(&mut self, source: &str, ts: i64, seq: Option<u64>) -> i64 {
        let ts = match self.stamps.get(source) {
            Some(&(last, last_seq)) if ts < last || (ts == last && seq < last_seq) => {
                if seq >= last_seq { last } else { last + 1 }
            }
            _ => ts,
        };
        self.stamps.insert(source.to_string(), (ts, seq));
        ts
    }

    fn raw(&mut self, out: &mut Output, source: String, ts: i64, payload: &[u8]) {
        let ingest_seq = self.next_seq(Zone::Raw, &source);
        out.records.push(Record::Raw {
            source,
            record: RawRecord {
                ts_received: ts,
                ingest_seq,
                payload: payload.to_vec(),
            },
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn emit(
        &mut self,
        out: &mut Output,
        source: proto::SourceId,
        ts_event: i64,
        source_seq: Option<u64>,
        payload: envelope::Payload,
        ts_received: i64,
        provenance: Provenance,
    ) {
        let key = source_key(&source);
        let ts_available = self.stamp(&key, ts_received, source_seq);
        let ingest_seq = self.next_seq(Zone::Normalized, &key);
        out.records.push(Record::Normalized {
            source: key,
            envelope: proto::Envelope {
                ts_event,
                ts_received: Some(ts_received),
                ts_available,
                source: Some(source),
                source_seq,
                ingest_seq,
                provenance: provenance as i32,
                payload: Some(payload),
            },
        });
    }

    fn emit_normalized(
        &mut self,
        out: &mut Output,
        n: Normalized,
        ts_received: i64,
        provenance: Provenance,
    ) {
        self.emit(
            out,
            n.source,
            n.ts_event,
            n.source_seq,
            envelope::Payload::Market(n.event),
            ts_received,
            provenance,
        );
    }

    fn emit_market(
        &mut self,
        out: &mut Output,
        source: proto::SourceId,
        ts_event: i64,
        seq: Option<u64>,
        e: market_event::Event,
        ts: i64,
    ) {
        self.emit(
            out,
            source,
            ts_event,
            seq,
            envelope::Payload::Market(proto::MarketEvent { event: Some(e) }),
            ts,
            Provenance::Live,
        );
    }

    /// Emite un `Gap` de `source` en `[from, to)` detectado en `ts`.
    #[allow(clippy::too_many_arguments)]
    fn emit_gap(
        &mut self,
        out: &mut Output,
        source: proto::SourceId,
        from: i64,
        to: i64,
        reason: GapReason,
        detail: Option<String>,
        ts: i64,
    ) -> GapInfo {
        let key = source_key(&source);
        let mut h = blake3::Hasher::new();
        h.update(key.as_bytes());
        h.update(&from.to_le_bytes());
        h.update(&to.to_le_bytes());
        h.update(&(reason as i32).to_le_bytes());
        let mut random = [0u8; 16];
        random.copy_from_slice(&h.finalize().as_bytes()[..16]);
        let id = Ulid::from_parts(
            u64::try_from(ts / 1_000_000).unwrap_or(0),
            u128::from_be_bytes(random),
        );
        let to = to.max(from);
        self.emit(
            out,
            source.clone(),
            ts,
            None,
            envelope::Payload::Gap(proto::Gap {
                gap_id: Some(id.into()),
                source: Some(source),
                from_ts: from,
                to_ts: to,
                reason: reason as i32,
                detail,
            }),
            ts,
            Provenance::Live,
        );
        GapInfo { id, from, to }
    }

    #[allow(clippy::too_many_arguments)]
    fn emit_recovery(
        &mut self,
        out: &mut Output,
        source: proto::SourceId,
        gap: &GapInfo,
        range: Option<(i64, i64)>,
        method: RecoveryMethod,
        result: RecoveryResult,
        ts: i64,
    ) {
        self.emit(
            out,
            source,
            ts,
            None,
            envelope::Payload::GapRecovery(proto::GapRecovery {
                gap_id: Some(gap.id.into()),
                recovered_from: range.map(|r| r.0),
                recovered_to: range.map(|r| r.1),
                method: method as i32,
                result: result as i32,
            }),
            ts,
            Provenance::Live,
        );
    }

    /// Fuentes del WS de un instrumento (sin el libro L2, que tiene su propio manejo).
    fn ws_sources(i: &Instrument) -> Vec<MarketDataKind> {
        let mut kinds = vec![MarketDataKind::Trades];
        if i.book == BookMode::Top {
            kinds.push(MarketDataKind::TopOfBook);
        }
        if i.market == Market::UsdM {
            kinds.extend([
                MarketDataKind::MarkPrice,
                MarketDataKind::IndexPrice,
                MarketDataKind::Funding,
                MarketDataKind::Liquidations,
            ]);
        }
        kinds
    }

    fn on_connected(&mut self, market: Market, ts: i64, out: &mut Output) {
        let conn = self.conns.entry(market).or_default();
        conn.up = true;
        conn.last_msg = Some(ts);
        conn.reconnect_requested = false;
        let down = conn.down.take();
        let instruments: Vec<Instrument> = self.cfg.instruments(market).cloned().collect();
        for i in &instruments {
            if let Some(down) = &down {
                for kind in Self::ws_sources(i) {
                    let source = bender_venue_binance::source(market, &i.symbol, kind);
                    let key = source_key(&source);
                    if down.reason == GapReason::ProcessDown && !self.known.contains(&key) {
                        continue;
                    }
                    let gap = self.emit_gap(
                        out,
                        source.clone(),
                        down.from,
                        ts,
                        down.reason,
                        down.detail.clone(),
                        ts,
                    );
                    if kind == MarketDataKind::Trades {
                        let st = self
                            .trades
                            .get_mut(&(market, i.symbol.clone()))
                            .expect("estado de trades");
                        if st.last_id.is_some() {
                            st.awaiting.push(gap);
                        } else {
                            self.emit_recovery(
                                out,
                                source,
                                &gap,
                                None,
                                RecoveryMethod::RestById,
                                RecoveryResult::None,
                                ts,
                            );
                        }
                    } else {
                        self.emit_recovery(
                            out,
                            source,
                            &gap,
                            None,
                            RecoveryMethod::None,
                            RecoveryResult::None,
                            ts,
                        );
                    }
                }
            }
            let book_key = source_key(&bender_venue_binance::source(
                market,
                &i.symbol,
                MarketDataKind::Book,
            ));
            if let Some(bs) = self.books.get_mut(&(market, i.symbol.clone())) {
                if let Some(down) = &down
                    && (down.reason != GapReason::ProcessDown || self.known.contains(&book_key))
                    && bs.pending.is_none()
                {
                    bs.pending = Some(down.clone());
                }
                if !bs.requested {
                    bs.requested = true;
                    out.requests.push(Request::Snapshot {
                        market,
                        symbol: i.symbol.clone(),
                    });
                }
            }
        }
    }

    fn on_disconnected(&mut self, market: Market, ts: i64, reason: GapReason, detail: String) {
        let conn = self.conns.entry(market).or_default();
        if !conn.up {
            return;
        }
        conn.up = false;
        let down = Down {
            from: conn.last_msg.unwrap_or(ts),
            reason,
            detail: Some(detail),
        };
        conn.down = Some(down.clone());
        for ((m, _), bs) in self.books.iter_mut() {
            if *m != market {
                continue;
            }
            if bs.book.is_synced() || bs.pending.is_none() {
                bs.pending = Some(down.clone());
            }
            bs.book.invalidate();
            bs.requested = false;
            bs.retries = 0;
            bs.retry_at = None;
        }
    }

    fn on_message(&mut self, market: Market, ts: i64, raw: Vec<u8>, out: &mut Output) {
        if let Some(conn) = self.conns.get_mut(&market) {
            conn.last_msg = Some(ts);
        }
        let parsed = match bender_venue_binance::parse(&raw) {
            Ok(p) => p,
            Err(_) => {
                // Respuestas de suscripción, mensajes nuevos: quedan en la cruda.
                self.raw(out, raw_key(market, None, "ws:other"), ts, &raw);
                return;
            }
        };
        let stream = match &parsed {
            Parsed::AggTrade(_) => "ws:aggTrade",
            Parsed::Depth(_) => "ws:depth",
            Parsed::BookTicker(_) => "ws:bookTicker",
            Parsed::MarkPrice(_) => "ws:markPrice",
            Parsed::ForceOrder(_) => "ws:forceOrder",
        };
        let symbol = parsed.symbol().to_ascii_uppercase();
        self.raw(out, raw_key(market, Some(&symbol), stream), ts, &raw);
        match parsed {
            Parsed::AggTrade(t) => self.on_trade(market, &symbol, &t, ts, out),
            Parsed::Depth(d) => {
                let Some(bs) = self.books.get_mut(&(market, symbol.clone())) else {
                    return;
                };
                let outcome = bs.book.on_diff(&d);
                let source = bender_venue_binance::source(market, &symbol, MarketDataKind::Book);
                match outcome {
                    Ok(DiffOutcome::Applied(delta)) => {
                        bs.last_ts = Some(ts);
                        if let Some(delta) = delta {
                            self.emit_market(
                                out,
                                source,
                                ms(d.event_time),
                                Some(d.final_update_id),
                                market_event::Event::BookDelta(delta),
                                ts,
                            );
                        }
                    }
                    Ok(DiffOutcome::Stale | DiffOutcome::Buffered) => {}
                    Ok(DiffOutcome::Gap { expected, got }) => {
                        if bs.pending.is_none() {
                            bs.pending = Some(Down {
                                from: bs.last_ts.unwrap_or(ts),
                                reason: GapReason::SequenceBreak,
                                detail: Some(format!("libro: esperaba {expected}, llegó {got}")),
                            });
                        }
                        if !bs.requested {
                            bs.requested = true;
                            out.requests.push(Request::Snapshot { market, symbol });
                        }
                    }
                    Err(_) => {}
                }
            }
            other => {
                if let Ok(events) = bender_venue_binance::normalize(market, &other, ts) {
                    for n in events {
                        self.emit_normalized(out, n, ts, Provenance::Live);
                    }
                }
            }
        }
    }

    fn on_trade(&mut self, market: Market, symbol: &str, t: &AggTrade, ts: i64, out: &mut Output) {
        let key = (market, symbol.to_string());
        let source = bender_venue_binance::source(market, symbol, MarketDataKind::Trades);
        let Some(st) = self.trades.get_mut(&key) else {
            return;
        };
        if st.last_id.is_some_and(|l| t.agg_id <= l) {
            return; // ya visto
        }
        let mut gaps = std::mem::take(&mut st.awaiting);
        let last_id = st.last_id;
        let last_ts = st.last_ts;
        if gaps.is_empty()
            && let Some(l) = last_id
            && t.agg_id > l + 1
        {
            let detail = format!("ids de trade: esperaba {}, llegó {}", l + 1, t.agg_id);
            gaps.push(self.emit_gap(
                out,
                source.clone(),
                last_ts.unwrap_or(ts),
                ts,
                GapReason::SequenceBreak,
                Some(detail),
                ts,
            ));
        }
        if !gaps.is_empty() {
            self.start_recovery(market, symbol, gaps, last_id, t.agg_id, ts, out);
        }
        let parsed = Parsed::AggTrade(t.clone());
        if let Ok(events) = bender_venue_binance::normalize(market, &parsed, ts) {
            for n in events {
                self.emit_normalized(out, n, ts, Provenance::Live);
            }
        }
        let st = self.trades.get_mut(&key).expect("estado de trades");
        st.last_id = Some(t.agg_id);
        st.last_ts = Some(ts);
    }

    #[allow(clippy::too_many_arguments)]
    fn start_recovery(
        &mut self,
        market: Market,
        symbol: &str,
        gaps: Vec<GapInfo>,
        last_id: Option<u64>,
        until_id: u64,
        ts: i64,
        out: &mut Output,
    ) {
        let source = bender_venue_binance::source(market, symbol, MarketDataKind::Trades);
        let Some(last) = last_id else {
            for g in &gaps {
                self.emit_recovery(
                    out,
                    source.clone(),
                    g,
                    None,
                    RecoveryMethod::RestById,
                    RecoveryResult::None,
                    ts,
                );
            }
            return;
        };
        let rec = Recovery {
            gaps,
            start_id: last + 1,
            next_id: last + 1,
            until_id,
            pages: 0,
            written: 0,
            skipped: false,
            prefix_last_ts: None,
        };
        if rec.next_id >= rec.until_id {
            // No se perdió ningún trade: la continuidad de ids ya está.
            self.finish_recovery(market, symbol, rec, ts, out);
            return;
        }
        let st = self
            .trades
            .get_mut(&(market, symbol.to_string()))
            .expect("estado de trades");
        st.recoveries.push_back(rec);
        if st.recoveries.len() == 1 {
            out.requests.push(Request::AggTrades {
                market,
                symbol: symbol.to_string(),
                from_id: last + 1,
                limit: self.cfg.recovery_page,
            });
        }
    }

    fn on_agg_trades(
        &mut self,
        market: Market,
        symbol: &str,
        from_id: u64,
        ts: i64,
        response: Result<Vec<u8>, String>,
        out: &mut Output,
    ) {
        if let Ok(raw) = &response {
            self.raw(
                out,
                raw_key(market, Some(symbol), "rest:aggTrades"),
                ts,
                raw,
            );
        }
        let key = (market, symbol.to_string());
        let Some(st) = self.trades.get_mut(&key) else {
            return;
        };
        let Some(mut rec) = st.recoveries.pop_front() else {
            return;
        };
        if rec.next_id != from_id {
            st.recoveries.push_front(rec);
            return; // respuesta vieja
        }
        let trades = response
            .ok()
            .and_then(|raw| bender_venue_binance::parse_rest_agg_trades(&raw).ok());
        let mut done = trades.is_none();
        let mut trades = trades.unwrap_or_default();
        trades.sort_by_key(|t| t.agg_id);
        let mut progressed = false;
        for t in &trades {
            if t.agg_id < rec.next_id {
                continue;
            }
            if t.agg_id >= rec.until_id {
                done = true;
                break;
            }
            let Ok(n) = bender_venue_binance::rest_agg_trade(market, symbol, t) else {
                continue;
            };
            if t.agg_id != rec.next_id {
                rec.skipped = true;
            }
            if !rec.skipped {
                rec.prefix_last_ts = Some(n.ts_event);
            }
            self.emit_normalized(out, n, ts, Provenance::Recovered);
            rec.next_id = t.agg_id + 1;
            rec.written += 1;
            progressed = true;
        }
        rec.pages += 1;
        if !progressed || rec.next_id >= rec.until_id || rec.pages >= self.cfg.max_recovery_pages {
            done = true;
        }
        if done {
            self.finish_recovery(market, symbol, rec, ts, out);
            let st = self.trades.get_mut(&key).expect("estado de trades");
            if let Some(next) = st.recoveries.front() {
                out.requests.push(Request::AggTrades {
                    market,
                    symbol: symbol.to_string(),
                    from_id: next.next_id,
                    limit: self.cfg.recovery_page,
                });
            }
        } else {
            out.requests.push(Request::AggTrades {
                market,
                symbol: symbol.to_string(),
                from_id: rec.next_id,
                limit: self.cfg.recovery_page,
            });
            self.trades
                .get_mut(&key)
                .expect("estado de trades")
                .recoveries
                .push_front(rec);
        }
    }

    fn finish_recovery(
        &mut self,
        market: Market,
        symbol: &str,
        rec: Recovery,
        ts: i64,
        out: &mut Output,
    ) {
        let source = bender_venue_binance::source(market, symbol, MarketDataKind::Trades);
        let full = !rec.skipped && rec.next_id >= rec.until_id;
        // Tramo continuo recuperado desde el inicio del primer hueco.
        let prefix = rec
            .prefix_last_ts
            .filter(|_| rec.next_id > rec.start_id)
            .map(|last| (rec.gaps[0].from, last + 1));
        for g in &rec.gaps {
            let (range, result) = if full {
                (Some((g.from, g.to)), RecoveryResult::Full)
            } else {
                let part = prefix
                    .map(|(a, b)| (a.max(g.from), b.min(g.to)))
                    .filter(|(a, b)| a < b);
                match part {
                    Some(r) if r == (g.from, g.to) => (Some(r), RecoveryResult::Full),
                    Some(r) => (Some(r), RecoveryResult::Partial),
                    None if rec.written > 0 && prefix.is_none() => (None, RecoveryResult::Partial),
                    None => (None, RecoveryResult::None),
                }
            };
            self.emit_recovery(
                out,
                source.clone(),
                g,
                range,
                RecoveryMethod::RestById,
                result,
                ts,
            );
        }
    }

    fn on_snapshot(
        &mut self,
        market: Market,
        symbol: &str,
        ts: i64,
        raw: Vec<u8>,
        out: &mut Output,
    ) {
        self.raw(out, raw_key(market, Some(symbol), "rest:depth"), ts, &raw);
        let key = (market, symbol.to_string());
        let Some(bs) = self.books.get_mut(&key) else {
            return;
        };
        bs.requested = false;
        let connected = self.conns.get(&market).is_some_and(|c| c.up);
        let outcome = bender_venue_binance::parse_depth_snapshot(&raw)
            .and_then(|s| bs.book.on_snapshot(&s).map(|o| (s, o)));
        let (snap, snapshot, deltas) = match outcome {
            Ok((s, SnapshotOutcome::Synced { snapshot, deltas })) if connected => {
                (s, snapshot, deltas)
            }
            Ok((_, SnapshotOutcome::Synced { .. })) => {
                // Llegó con la conexión caída: el próximo `Connected` pide otro.
                bs.book.invalidate();
                return;
            }
            Ok((_, SnapshotOutcome::Retry { .. })) | Err(_) => {
                // Se pide otro en el tick, con backoff (y mientras tanto se juntan
                // deltas).
                bs.retry_at = Some(ts + snapshot_backoff(bs.retries));
                bs.retries += 1;
                return;
            }
        };
        bs.retries = 0;
        bs.retry_at = None;
        let pending = bs.pending.take();
        bs.last_ts = Some(ts);
        bs.next_periodic = ts + self.cfg.book_snapshot_every_ns;
        let source = bender_venue_binance::source(market, symbol, MarketDataKind::Book);
        if let Some(down) = pending {
            let gap = self.emit_gap(
                out,
                source.clone(),
                down.from,
                ts,
                down.reason,
                down.detail,
                ts,
            );
            self.emit_recovery(
                out,
                source.clone(),
                &gap,
                None,
                RecoveryMethod::Resnapshot,
                RecoveryResult::None,
                ts,
            );
        }
        self.emit_market(
            out,
            source.clone(),
            snap.event_time.map_or(ts, ms),
            Some(snap.last_update_id),
            market_event::Event::BookSnapshot(snapshot),
            ts,
        );
        for (event_time, delta) in deltas {
            let seq = delta.venue_seq;
            self.emit_market(
                out,
                source.clone(),
                ms(event_time),
                seq,
                market_event::Event::BookDelta(delta),
                ts,
            );
        }
    }

    fn on_tick(&mut self, ts: i64, out: &mut Output) {
        self.emit(
            out,
            clock_source(),
            ts,
            None,
            envelope::Payload::Heartbeat(proto::Heartbeat {}),
            ts,
            Provenance::Live,
        );
        for (&market, conn) in self.conns.iter_mut() {
            if conn.up
                && !conn.reconnect_requested
                && conn
                    .last_msg
                    .is_some_and(|t| ts - t > self.cfg.stale_after_ns)
            {
                conn.reconnect_requested = true;
                out.requests.push(Request::Reconnect {
                    market,
                    reason: GapReason::Stale,
                    detail: format!(
                        "sin mensajes por más de {} ms",
                        self.cfg.stale_after_ns / 1_000_000
                    ),
                });
            }
        }
        let due: Vec<(Market, String)> = self
            .books
            .iter()
            .filter(|(_, bs)| bs.book.is_synced() && ts >= bs.next_periodic)
            .map(|(k, _)| k.clone())
            .collect();
        for (market, symbol) in due {
            let bs = self
                .books
                .get_mut(&(market, symbol.clone()))
                .expect("libro");
            bs.next_periodic = ts + self.cfg.book_snapshot_every_ns;
            let seq = bs.book.last_update_id();
            if let Some(snapshot) = bs.book.snapshot() {
                let source = bender_venue_binance::source(market, &symbol, MarketDataKind::Book);
                self.emit_market(
                    out,
                    source,
                    ts,
                    seq,
                    market_event::Event::BookSnapshot(snapshot),
                    ts,
                );
            }
        }
        for ((market, symbol), bs) in self.books.iter_mut() {
            let up = self.conns.get(market).is_some_and(|c| c.up);
            if up && !bs.requested && bs.retry_at.is_some_and(|t| ts >= t) {
                bs.requested = true;
                bs.retry_at = None;
                out.requests.push(Request::Snapshot {
                    market: *market,
                    symbol: symbol.clone(),
                });
            }
        }
        if ts >= self.next_open_interest {
            self.next_open_interest = ts + self.cfg.open_interest_every_ns;
            for i in self.cfg.instruments(Market::UsdM) {
                out.requests.push(Request::OpenInterest {
                    symbol: i.symbol.clone(),
                });
            }
        }
    }
}
