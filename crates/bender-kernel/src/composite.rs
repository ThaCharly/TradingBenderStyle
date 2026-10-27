//! Velas compuestas multi-venue (doc 05 §6, PLAN §4.1).
//!
//! - **Definición** versionada: un id, una versión, las fuentes de trades incluidas y la
//!   regla de precio. Cada vela registra id y versión.
//! - **Volúmenes** en unidades del activo base: los trades ya llegan así (la conversión
//!   desde contratos la hace el adaptador con [`crate::spec::SpecHistory::to_base`]). El
//!   volumen de cada fuente se conserva dentro de la vela.
//! - **Precio**: con [`PriceRule::Vwap`] (default), OHLC sale de los trades de todas las
//!   fuentes y la vela informa su VWAP. Con [`PriceRule::Reference`], OHLC sale sólo de la
//!   fuente de referencia (si no operó en la vela, la vela no tiene precio) y los
//!   volúmenes siguen siendo de todas.
//! - **Cierre**: cuando todas las fuentes superaron el fin (`max ts_event − L ≥ fin` en
//!   cada una), o cuando el reloj (`ts_available` de trades o latidos) llega a
//!   `fin + L + M`. En el segundo caso, las fuentes atrasadas quedan listadas como
//!   incompletas. `M` = 5 s por defecto.
//! - **Tardíos**: como en las velas simples (revisión n+1). La lista de incompletas se
//!   recalcula, así que una fuente que se puso al día deja de figurar.

use std::collections::BTreeMap;

use bender_types::proto::Side;
use bender_types::{Decimal, NS_PER_DAY};

use crate::bars::{BarError, BarEventKind, BarState, Timeframe, TradeTick};

/// Escala del VWAP (decimales), con redondeo half-even.
pub const VWAP_SCALE: u32 = 12;

/// Regla de precio de una compuesta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PriceRule {
    /// OHLC de los trades de todas las fuentes, más el VWAP de la vela.
    Vwap,
    /// OHLC sólo de esta fuente (clave canónica).
    Reference(String),
}

/// Definición versionada de una compuesta, p. ej. "BTC/USD perps agregado" v1.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeDefinition {
    pub id: String,
    pub version: u32,
    /// Claves canónicas de las fuentes de trades incluidas.
    pub members: Vec<String>,
    pub price_rule: PriceRule,
}

/// Parámetros de la agregación compuesta.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompositeConfig {
    /// Demora permitida `L`, igual que en las velas simples.
    pub lateness_ns: i64,
    /// Espera máxima `M` por las fuentes atrasadas.
    pub max_wait_ns: i64,
    /// Cuánto se conservan las velas cerradas para poder revisarlas.
    pub revision_horizon_ns: i64,
}

impl Default for CompositeConfig {
    fn default() -> Self {
        Self {
            lateness_ns: 2_000_000_000,
            max_wait_ns: 5_000_000_000,
            revision_horizon_ns: NS_PER_DAY,
        }
    }
}

/// Volumen de una fuente dentro de la vela.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceVolume {
    pub volume_base: Decimal,
    pub volume_quote: Decimal,
    pub buy_volume_base: Decimal,
    pub sell_volume_base: Decimal,
    pub trade_count: u64,
}

/// Precios de la vela.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ohlc {
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub first_ts_event: i64,
    pub last_ts_event: i64,
}

/// Vela compuesta.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeBar {
    pub definition_id: String,
    pub definition_version: u32,
    pub timeframe: Timeframe,
    pub start: i64,
    pub end: i64,
    /// Ausente con [`PriceRule::Reference`] si la referencia no operó en la vela.
    pub prices: Option<Ohlc>,
    /// `volume_quote / volume_base`, con [`VWAP_SCALE`] decimales.
    pub vwap: Option<Decimal>,
    pub volume_base: Decimal,
    pub volume_quote: Decimal,
    pub buy_volume_base: Decimal,
    pub sell_volume_base: Decimal,
    pub trade_count: u64,
    pub per_source: BTreeMap<String, SourceVolume>,
    /// Fuentes que no habían superado el fin cuando la vela cerró (o se revisó).
    pub incomplete_sources: Vec<String>,
    pub revision: u32,
    pub state: BarState,
}

/// Evento de salida.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompositeEvent {
    pub kind: BarEventKind,
    pub bar: CompositeBar,
    pub ts_available: i64,
}

impl CompositeBar {
    fn new(def: &CompositeDefinition, timeframe: Timeframe, start: i64) -> Self {
        Self {
            definition_id: def.id.clone(),
            definition_version: def.version,
            timeframe,
            start,
            end: timeframe.bucket_end(start),
            prices: None,
            vwap: None,
            volume_base: Decimal::ZERO,
            volume_quote: Decimal::ZERO,
            buy_volume_base: Decimal::ZERO,
            sell_volume_base: Decimal::ZERO,
            trade_count: 0,
            per_source: BTreeMap::new(),
            incomplete_sources: Vec::new(),
            revision: 1,
            state: BarState::Forming,
        }
    }

    fn apply(
        &mut self,
        source: &str,
        t: &TradeTick,
        notional: Decimal,
        priced: bool,
    ) -> Result<(), BarError> {
        if priced {
            match &mut self.prices {
                None => {
                    self.prices = Some(Ohlc {
                        open: t.price,
                        high: t.price,
                        low: t.price,
                        close: t.price,
                        first_ts_event: t.ts_event,
                        last_ts_event: t.ts_event,
                    })
                }
                Some(p) => {
                    if t.ts_event < p.first_ts_event {
                        p.first_ts_event = t.ts_event;
                        p.open = t.price;
                    }
                    if t.ts_event >= p.last_ts_event {
                        p.last_ts_event = t.ts_event;
                        p.close = t.price;
                    }
                    if t.price > p.high {
                        p.high = t.price;
                    }
                    if t.price < p.low {
                        p.low = t.price;
                    }
                }
            }
        }
        let (buy, sell) = match t.aggressor {
            Side::Buy => (t.qty_base, Decimal::ZERO),
            Side::Sell => (Decimal::ZERO, t.qty_base),
            Side::Unspecified => (Decimal::ZERO, Decimal::ZERO),
        };
        let v = self.per_source.entry(source.to_string()).or_default();
        v.volume_base = v.volume_base.checked_add(t.qty_base)?;
        v.volume_quote = v.volume_quote.checked_add(notional)?;
        v.buy_volume_base = v.buy_volume_base.checked_add(buy)?;
        v.sell_volume_base = v.sell_volume_base.checked_add(sell)?;
        v.trade_count += 1;
        self.volume_base = self.volume_base.checked_add(t.qty_base)?;
        self.volume_quote = self.volume_quote.checked_add(notional)?;
        self.buy_volume_base = self.buy_volume_base.checked_add(buy)?;
        self.sell_volume_base = self.sell_volume_base.checked_add(sell)?;
        self.trade_count += 1;
        self.vwap = if self.volume_base.is_zero() {
            None
        } else {
            Some(
                self.volume_quote
                    .checked_div_round(self.volume_base, VWAP_SCALE)?,
            )
        };
        Ok(())
    }
}

#[derive(Clone, Debug)]
struct TfState {
    tf: Timeframe,
    forming: BTreeMap<i64, CompositeBar>,
    closed: BTreeMap<i64, Vec<CompositeBar>>,
}

/// Agregador de una vela compuesta en N temporalidades.
#[derive(Clone, Debug)]
pub struct CompositeAggregator {
    def: CompositeDefinition,
    config: CompositeConfig,
    tfs: Vec<TfState>,
    /// `max ts_event` por fuente.
    max_ts_event: BTreeMap<String, i64>,
    /// Mayor `ts_available` visto (trades y latidos).
    clock: Option<i64>,
    late_dropped: u64,
}

impl CompositeAggregator {
    pub fn new(
        def: CompositeDefinition,
        timeframes: &[Timeframe],
        config: CompositeConfig,
    ) -> Self {
        Self {
            def,
            config,
            tfs: timeframes
                .iter()
                .map(|&tf| TfState {
                    tf,
                    forming: BTreeMap::new(),
                    closed: BTreeMap::new(),
                })
                .collect(),
            max_ts_event: BTreeMap::new(),
            clock: None,
            late_dropped: 0,
        }
    }

    pub fn definition(&self) -> &CompositeDefinition {
        &self.def
    }

    pub fn late_dropped(&self) -> u64 {
        self.late_dropped
    }

    pub fn forming(&self, tf: Timeframe) -> impl Iterator<Item = &CompositeBar> {
        self.tfs
            .iter()
            .filter(move |s| s.tf == tf)
            .flat_map(|s| s.forming.values())
    }

    pub fn revisions(&self, tf: Timeframe, start: i64) -> Option<&[CompositeBar]> {
        self.tfs
            .iter()
            .find(|s| s.tf == tf)
            .and_then(|s| s.closed.get(&start))
            .map(Vec::as_slice)
    }

    /// Fuentes que todavía no superaron `end` (su `max ts_event − L < end`).
    fn behind(&self, end: i64) -> Vec<String> {
        let l = self.config.lateness_ns;
        self.def
            .members
            .iter()
            .filter(|m| self.max_ts_event.get(*m).is_none_or(|t| t - l < end))
            .cloned()
            .collect()
    }

    fn ready(&self, end: i64) -> bool {
        let forced = self
            .clock
            .is_some_and(|c| c >= end + self.config.lateness_ns + self.config.max_wait_ns);
        forced || self.behind(end).is_empty()
    }

    /// Agrega un trade de `source` (clave canónica). Los trades de fuentes que no son
    /// miembros se ignoran.
    pub fn on_trade(
        &mut self,
        source: &str,
        trade: &TradeTick,
        ts_available: i64,
        out: &mut Vec<CompositeEvent>,
    ) -> Result<(), BarError> {
        if !self.def.members.iter().any(|m| m == source) {
            return Ok(());
        }
        let notional = trade.price.checked_mul(trade.qty_base)?;
        let priced = match &self.def.price_rule {
            PriceRule::Vwap => true,
            PriceRule::Reference(r) => r == source,
        };
        let horizon = self.config.revision_horizon_ns;
        let clock_before = self.clock;
        let mut dropped = false;
        for i in 0..self.tfs.len() {
            let tf = self.tfs[i].tf;
            let start = tf.bucket_start(trade.ts_event);
            let end = tf.bucket_end(start);
            if let Some(bar) = self.tfs[i].forming.get_mut(&start) {
                bar.apply(source, trade, notional, priced)?;
                continue;
            }
            if self.tfs[i].closed.contains_key(&start) || self.ready(end) {
                if clock_before.is_some_and(|c| end.saturating_add(horizon) < c)
                    && !self.tfs[i].closed.contains_key(&start)
                {
                    dropped = true;
                    continue;
                }
                // Vela ya cerrada (o que ya habría cerrado): revisión, o primera versión cerrada.
                let previous = self.tfs[i]
                    .closed
                    .get(&start)
                    .and_then(|h| h.last())
                    .cloned();
                let mut bar = previous
                    .clone()
                    .unwrap_or_else(|| CompositeBar::new(&self.def, tf, start));
                bar.apply(source, trade, notional, priced)?;
                // La fuente del trade cuenta como al día para esta vela si su marca de agua,
                // contando este trade, ya pasó el fin.
                let mut max_ts = self.max_ts_event.clone();
                let entry = max_ts.entry(source.to_string()).or_insert(trade.ts_event);
                *entry = (*entry).max(trade.ts_event);
                let l = self.config.lateness_ns;
                bar.incomplete_sources = self
                    .def
                    .members
                    .iter()
                    .filter(|m| max_ts.get(*m).is_none_or(|t| t - l < end))
                    .cloned()
                    .collect();
                bar.state = BarState::Closed;
                let kind = if previous.is_some() {
                    bar.revision += 1;
                    BarEventKind::Revised
                } else {
                    BarEventKind::Closed
                };
                self.tfs[i]
                    .closed
                    .entry(start)
                    .or_default()
                    .push(bar.clone());
                out.push(CompositeEvent {
                    kind,
                    bar,
                    ts_available,
                });
                continue;
            }
            let mut bar = CompositeBar::new(&self.def, tf, start);
            bar.apply(source, trade, notional, priced)?;
            self.tfs[i].forming.insert(start, bar);
        }
        if dropped {
            self.late_dropped += 1;
        }
        let entry = self
            .max_ts_event
            .entry(source.to_string())
            .or_insert(trade.ts_event);
        *entry = (*entry).max(trade.ts_event);
        self.tick(ts_available, out);
        Ok(())
    }

    /// Latido (o cualquier evento que haga avanzar el reloj): puede forzar cierres.
    pub fn on_heartbeat(&mut self, ts_available: i64, out: &mut Vec<CompositeEvent>) {
        self.tick(ts_available, out);
    }

    fn tick(&mut self, ts_available: i64, out: &mut Vec<CompositeEvent>) {
        self.clock = Some(self.clock.map_or(ts_available, |c| c.max(ts_available)));
        let horizon = self.config.revision_horizon_ns;
        let clock = self.clock.expect("reloj");
        for i in 0..self.tfs.len() {
            while let Some((&start, bar)) = self.tfs[i].forming.first_key_value() {
                let end = bar.end;
                if !self.ready(end) {
                    break;
                }
                let incomplete = self.behind(end);
                let mut bar = self.tfs[i].forming.remove(&start).expect("existe");
                bar.state = BarState::Closed;
                bar.incomplete_sources = incomplete;
                out.push(CompositeEvent {
                    kind: BarEventKind::Closed,
                    bar: bar.clone(),
                    ts_available,
                });
                self.tfs[i].closed.insert(start, vec![bar]);
            }
            while let Some(entry) = self.tfs[i].closed.first_entry() {
                let end = entry.get().last().map_or(i64::MIN, |b| b.end);
                if end.saturating_add(horizon) >= clock {
                    break;
                }
                entry.remove();
            }
        }
    }
}
