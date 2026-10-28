//! Binance simulado para probar el núcleo con tiempo simulado.

#![allow(dead_code)]

use std::collections::BTreeMap;

use bender_recorder::core::{BookMode, Core, CoreConfig, Input, Instrument, Link, Record, Request};
use bender_types::proto::{self, envelope};
use bender_venue_binance::Market;

pub const S: i64 = 1_000_000_000;
pub const MS: i64 = 1_000_000;

pub fn config(instruments: Vec<Instrument>) -> CoreConfig {
    CoreConfig {
        instruments,
        top_n: 20,
        book_snapshot_every_ns: 60 * S,
        stale_after_ns: 10 * S,
        open_interest_every_ns: 60 * S,
        recovery_page: 1000,
        max_recovery_pages: 100,
    }
}

pub fn instrument(market: Market, symbol: &str, book: BookMode) -> Instrument {
    Instrument {
        market,
        symbol: symbol.into(),
        book,
    }
}

/// Un trade del exchange: id, instante (ms), precio.
#[derive(Clone, Copy, Debug)]
pub struct Trade {
    pub id: u64,
    pub ts_ms: i64,
    pub price: i64,
}

impl Trade {
    pub fn ws(&self, symbol: &str) -> Vec<u8> {
        format!(
            r#"{{"stream":"{s}@aggTrade","data":{{"e":"aggTrade","E":{t},"s":"{S}","a":{id},"p":"{p}.00","q":"0.010","f":{id},"l":{id},"T":{t},"m":{m}}}}}"#,
            s = symbol.to_ascii_lowercase(),
            S = symbol,
            t = self.ts_ms,
            id = self.id,
            p = self.price,
            m = self.id.is_multiple_of(2)
        )
        .into_bytes()
    }

    pub fn rest(&self) -> String {
        format!(
            r#"{{"a":{id},"p":"{p}.00","q":"0.010","f":{id},"l":{id},"T":{t},"m":{m}}}"#,
            id = self.id,
            p = self.price,
            t = self.ts_ms,
            m = self.id.is_multiple_of(2)
        )
    }
}

/// Trades `first..=last`, uno cada `step_ms` desde `start_ms`.
pub fn trades(first: u64, last: u64, start_ms: i64, step_ms: i64) -> Vec<Trade> {
    (first..=last)
        .map(|id| Trade {
            id,
            ts_ms: start_ms + (id - first) as i64 * step_ms,
            price: 100_000 + (id % 37) as i64,
        })
        .collect()
}

/// Snapshot del libro: `lastUpdateId`, compras y ventas `(precio, cantidad)`.
pub type Snapshot = (u64, Vec<(i64, i64)>, Vec<(i64, i64)>);

/// Exchange simulado: guarda todos los trades y contesta los pedidos REST.
pub struct Sim {
    pub core: Core,
    pub records: Vec<Record>,
    pub requests: Vec<Request>,
    pub symbol: String,
    pub market: Market,
    pub all_trades: Vec<Trade>,
    /// El REST sólo devuelve trades con id ≤ este (para recuperaciones parciales).
    pub rest_available_until: u64,
    /// Snapshot que devuelve el REST del libro: `lastUpdateId` y niveles.
    pub snapshot: Option<Snapshot>,
    /// Instante simulado de la próxima respuesta REST.
    pub rest_latency_ns: i64,
}

impl Sim {
    pub fn new(core: Core, market: Market, symbol: &str) -> Self {
        Self {
            core,
            records: Vec::new(),
            requests: Vec::new(),
            symbol: symbol.into(),
            market,
            all_trades: Vec::new(),
            rest_available_until: u64::MAX,
            snapshot: None,
            rest_latency_ns: 50 * MS,
        }
    }

    /// Procesa una entrada y contesta los pedidos REST que genere (en cadena).
    pub fn input(&mut self, input: Input) {
        let mut queue = vec![input];
        while let Some(input) = queue.pop() {
            let ts = match &input {
                Input::Connected { ts, .. }
                | Input::Disconnected { ts, .. }
                | Input::Message { ts, .. }
                | Input::Snapshot { ts, .. }
                | Input::AggTrades { ts, .. }
                | Input::AggTradesFailed { ts, .. }
                | Input::OpenInterest { ts, .. }
                | Input::Tick { ts } => *ts,
            };
            let out = self.core.handle(input);
            self.records.extend(out.records);
            for r in out.requests {
                self.requests.push(r.clone());
                let at = ts + self.rest_latency_ns;
                match r {
                    Request::AggTrades {
                        market,
                        symbol,
                        from_id,
                        limit,
                    } => {
                        let page: Vec<String> = self
                            .all_trades
                            .iter()
                            .filter(|t| t.id >= from_id && t.id <= self.rest_available_until)
                            .take(limit as usize)
                            .map(Trade::rest)
                            .collect();
                        queue.push(Input::AggTrades {
                            market,
                            symbol,
                            from_id,
                            ts: at,
                            raw: format!("[{}]", page.join(",")).into_bytes(),
                        });
                    }
                    Request::Snapshot { market, symbol } => {
                        if let Some((id, bids, asks)) = &self.snapshot {
                            let lv = |v: &Vec<(i64, i64)>| {
                                v.iter()
                                    .map(|(p, q)| format!(r#"["{p}.00","{q}.000"]"#))
                                    .collect::<Vec<_>>()
                                    .join(",")
                            };
                            queue.push(Input::Snapshot {
                                market,
                                symbol,
                                ts: at,
                                raw: format!(
                                    r#"{{"lastUpdateId":{id},"bids":[{}],"asks":[{}]}}"#,
                                    lv(bids),
                                    lv(asks)
                                )
                                .into_bytes(),
                            });
                        }
                    }
                    Request::OpenInterest { .. } | Request::Reconnect { .. } => {}
                }
            }
        }
    }

    /// El exchange produce estos trades; si `deliver`, llegan por el WS (con 5 ms de
    /// latencia).
    pub fn trade(&mut self, t: Trade, deliver: bool) {
        self.all_trades.push(t);
        if deliver {
            let raw = t.ws(&self.symbol);
            let link = match self.market {
                Market::Spot => Link::Spot,
                Market::UsdM => Link::UsdmMarket,
            };
            self.input(Input::Message {
                link,
                ts: t.ts_ms * MS + 5 * MS,
                raw,
            });
        }
    }

    /// Se conectan todas las conexiones del mercado.
    pub fn connect(&mut self, ts: i64) {
        for &link in Link::of(self.market) {
            self.input(Input::Connected { link, ts });
        }
    }

    /// Se caen todas las conexiones del mercado.
    pub fn disconnect(&mut self, ts: i64, reason: proto::GapReason, detail: &str) {
        for &link in Link::of(self.market) {
            self.input(Input::Disconnected {
                link,
                ts,
                reason,
                detail: detail.into(),
            });
        }
    }

    pub fn tick(&mut self, ts: i64) {
        self.input(Input::Tick { ts });
    }

    pub fn normalized(&self) -> Vec<(String, proto::Envelope)> {
        self.records
            .iter()
            .filter_map(|r| match r {
                Record::Normalized { source, envelope } => Some((source.clone(), envelope.clone())),
                Record::Raw { .. } => None,
            })
            .collect()
    }

    pub fn of_source(&self, source: &str) -> Vec<proto::Envelope> {
        self.normalized()
            .into_iter()
            .filter(|(s, _)| s == source)
            .map(|(_, e)| e)
            .collect()
    }
}

pub fn is_trade(e: &proto::Envelope) -> bool {
    matches!(
        &e.payload,
        Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(proto::market_event::Event::Trade(_))
        }))
    )
}

pub fn gaps(events: &[proto::Envelope]) -> Vec<proto::Gap> {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            Some(envelope::Payload::Gap(g)) => Some(g.clone()),
            _ => None,
        })
        .collect()
}

pub fn recoveries(events: &[proto::Envelope]) -> Vec<(i64, proto::GapRecovery)> {
    events
        .iter()
        .filter_map(|e| match &e.payload {
            Some(envelope::Payload::GapRecovery(g)) => Some((e.ts_available, g.clone())),
            _ => None,
        })
        .collect()
}

/// Verifica el orden dentro de cada fuente: `(ts_available, source_seq)` no retrocede e
/// `ingest_seq` sube de a uno.
pub fn assert_per_source_order(records: &[Record]) {
    let mut last: BTreeMap<String, (i64, Option<u64>, u64)> = BTreeMap::new();
    for r in records {
        if let Record::Normalized { source, envelope } = r {
            let cur = (
                envelope.ts_available,
                envelope.source_seq,
                envelope.ingest_seq,
            );
            if let Some(prev) = last.get(source) {
                assert!(
                    (cur.0, cur.1) >= (prev.0, prev.1),
                    "{source}: {prev:?} → {cur:?}"
                );
                assert_eq!(cur.2, prev.2 + 1, "{source}: ingest_seq");
            }
            last.insert(source.clone(), cur);
        }
    }
}
