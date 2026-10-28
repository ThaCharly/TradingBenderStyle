//! Normalizador puro de los mensajes públicos de Binance.

use bender_types::proto::{self, MarketDataKind, envelope, market_event};
use bender_types::{Decimal, DecimalError};
use serde::Deserialize;

use crate::source;

/// Mercado de Binance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Market {
    Spot,
    /// Futuros perpetuos USDⓈ-M.
    UsdM,
}

/// Error al normalizar.
#[derive(Debug, PartialEq, Eq)]
pub enum NormalizeError {
    Json(String),
    Decimal(String),
    /// Mensaje válido pero de un tipo que este normalizador no maneja.
    Unsupported(String),
}

impl std::fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "JSON inválido: {e}"),
            Self::Decimal(e) => write!(f, "decimal inválido: {e}"),
            Self::Unsupported(e) => write!(f, "mensaje no soportado: {e}"),
        }
    }
}

impl std::error::Error for NormalizeError {}

impl From<DecimalError> for NormalizeError {
    fn from(e: DecimalError) -> Self {
        Self::Decimal(e.to_string())
    }
}

fn dec(s: &str) -> Result<Decimal, NormalizeError> {
    s.parse::<Decimal>().map_err(NormalizeError::from)
}

fn ms(ms: i64) -> i64 {
    ms * 1_000_000
}

/// `aggTrade` (spot y USDⓈ-M).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct AggTrade {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    /// Id del trade agregado: continuo dentro de un símbolo.
    #[serde(rename = "a")]
    pub agg_id: u64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub qty: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    /// El comprador es el maker → el agresor vendió.
    #[serde(rename = "m")]
    pub buyer_is_maker: bool,
}

/// `depthUpdate` (spot: sin `pu`; USDⓈ-M: con `pu`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DepthUpdate {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "U")]
    pub first_update_id: u64,
    #[serde(rename = "u")]
    pub final_update_id: u64,
    #[serde(rename = "pu", default)]
    pub prev_final_update_id: Option<u64>,
    #[serde(rename = "b")]
    pub bids: Vec<[String; 2]>,
    #[serde(rename = "a")]
    pub asks: Vec<[String; 2]>,
}

/// Snapshot REST del libro (`/api/v3/depth`, `/fapi/v1/depth`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct DepthSnapshot {
    #[serde(rename = "lastUpdateId")]
    pub last_update_id: u64,
    #[serde(rename = "E", default)]
    pub event_time: Option<i64>,
    pub bids: Vec<[String; 2]>,
    pub asks: Vec<[String; 2]>,
}

/// `bookTicker` (spot sin `E`; USDⓈ-M con `E`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct BookTicker {
    #[serde(rename = "u")]
    pub update_id: u64,
    #[serde(rename = "E", default)]
    pub event_time: Option<i64>,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "b")]
    pub bid: String,
    #[serde(rename = "B")]
    pub bid_qty: String,
    #[serde(rename = "a")]
    pub ask: String,
    #[serde(rename = "A")]
    pub ask_qty: String,
}

/// `markPriceUpdate` (USDⓈ-M): mark, index y funding en un mismo mensaje.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct MarkPriceUpdate {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "p")]
    pub mark: String,
    #[serde(rename = "i")]
    pub index: String,
    #[serde(rename = "r")]
    pub funding_rate: String,
    #[serde(rename = "T")]
    pub next_funding_time: i64,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ForceOrderInner {
    #[serde(rename = "s")]
    pub symbol: String,
    #[serde(rename = "S")]
    pub side: String,
    #[serde(rename = "ap")]
    pub avg_price: String,
    #[serde(rename = "z")]
    pub filled_qty: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
}

/// `forceOrder` (USDⓈ-M): liquidaciones.
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct ForceOrder {
    #[serde(rename = "E")]
    pub event_time: i64,
    #[serde(rename = "o")]
    pub order: ForceOrderInner,
}

/// Mensaje crudo ya interpretado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Parsed {
    AggTrade(AggTrade),
    Depth(DepthUpdate),
    BookTicker(BookTicker),
    MarkPrice(MarkPriceUpdate),
    ForceOrder(ForceOrder),
}

impl Parsed {
    pub fn symbol(&self) -> &str {
        match self {
            Self::AggTrade(m) => &m.symbol,
            Self::Depth(m) => &m.symbol,
            Self::BookTicker(m) => &m.symbol,
            Self::MarkPrice(m) => &m.symbol,
            Self::ForceOrder(m) => &m.order.symbol,
        }
    }
}

/// Interpreta un mensaje del WS, directo o envuelto por un stream combinado
/// (`{"stream": …, "data": {…}}`).
pub fn parse(raw: &[u8]) -> Result<Parsed, NormalizeError> {
    let value: serde_json::Value =
        serde_json::from_slice(raw).map_err(|e| NormalizeError::Json(e.to_string()))?;
    let data = match value.get("data") {
        Some(d) if value.get("stream").is_some() => d.clone(),
        _ => value,
    };
    let event = data.get("e").and_then(|e| e.as_str()).map(str::to_string);
    let json = |e: serde_json::Error| NormalizeError::Json(e.to_string());
    match event.as_deref() {
        Some("aggTrade") => Ok(Parsed::AggTrade(
            serde_json::from_value(data).map_err(json)?,
        )),
        Some("depthUpdate") => Ok(Parsed::Depth(serde_json::from_value(data).map_err(json)?)),
        Some("bookTicker") => Ok(Parsed::BookTicker(
            serde_json::from_value(data).map_err(json)?,
        )),
        Some("markPriceUpdate") => Ok(Parsed::MarkPrice(
            serde_json::from_value(data).map_err(json)?,
        )),
        Some("forceOrder") => Ok(Parsed::ForceOrder(
            serde_json::from_value(data).map_err(json)?,
        )),
        // El bookTicker de spot no trae "e": se reconoce por sus campos.
        None if data.get("u").is_some() && data.get("b").is_some() && data.get("A").is_some() => {
            Ok(Parsed::BookTicker(
                serde_json::from_value(data).map_err(json)?,
            ))
        }
        other => Err(NormalizeError::Unsupported(format!("{other:?}"))),
    }
}

/// Un evento canónico sin el sobre: fuente, instante del evento, secuencia del venue y el
/// evento. El grabador le agrega `ts_received`, `ts_available`, `ingest_seq` y
/// `provenance`.
#[derive(Clone, Debug, PartialEq)]
pub struct Normalized {
    pub source: proto::SourceId,
    pub ts_event: i64,
    pub source_seq: Option<u64>,
    pub event: proto::MarketEvent,
}

impl Normalized {
    pub fn into_envelope(
        self,
        ts_received: i64,
        ts_available: i64,
        ingest_seq: u64,
        provenance: proto::Provenance,
    ) -> proto::Envelope {
        proto::Envelope {
            ts_event: self.ts_event,
            ts_received: Some(ts_received),
            ts_available,
            source: Some(self.source),
            source_seq: self.source_seq,
            ingest_seq,
            provenance: provenance as i32,
            payload: Some(envelope::Payload::Market(self.event)),
        }
    }
}

fn event(e: market_event::Event) -> proto::MarketEvent {
    proto::MarketEvent { event: Some(e) }
}

/// Normaliza un mensaje que no depende de estado (todo salvo el libro, que lo maneja
/// [`crate::book::LocalBook`]). `ts_received` se usa como instante del evento cuando
/// el mensaje no lo trae (bookTicker de spot).
pub fn normalize(
    market: Market,
    parsed: &Parsed,
    ts_received: i64,
) -> Result<Vec<Normalized>, NormalizeError> {
    let n = |kind: MarketDataKind,
             symbol: &str,
             ts_event: i64,
             seq: Option<u64>,
             e: market_event::Event| Normalized {
        source: source(market, symbol, kind),
        ts_event,
        source_seq: seq,
        event: event(e),
    };
    Ok(match parsed {
        Parsed::AggTrade(t) => vec![n(
            MarketDataKind::Trades,
            &t.symbol,
            ms(t.trade_time),
            Some(t.agg_id),
            market_event::Event::Trade(proto::Trade {
                price: Some(dec(&t.price)?.into()),
                qty_base: Some(dec(&t.qty)?.into()),
                aggressor_side: if t.buyer_is_maker {
                    proto::Side::Sell
                } else {
                    proto::Side::Buy
                } as i32,
                venue_trade_id: Some(t.agg_id.to_string()),
            }),
        )],
        Parsed::BookTicker(b) => vec![n(
            MarketDataKind::TopOfBook,
            &b.symbol,
            b.event_time.map_or(ts_received, ms),
            Some(b.update_id),
            market_event::Event::TopOfBook(proto::TopOfBook {
                bid: Some(dec(&b.bid)?.into()),
                bid_qty: Some(dec(&b.bid_qty)?.into()),
                ask: Some(dec(&b.ask)?.into()),
                ask_qty: Some(dec(&b.ask_qty)?.into()),
            }),
        )],
        Parsed::MarkPrice(m) => {
            let ts = ms(m.event_time);
            vec![
                n(
                    MarketDataKind::MarkPrice,
                    &m.symbol,
                    ts,
                    None,
                    market_event::Event::MarkPrice(proto::MarkPrice {
                        price: Some(dec(&m.mark)?.into()),
                    }),
                ),
                n(
                    MarketDataKind::IndexPrice,
                    &m.symbol,
                    ts,
                    None,
                    market_event::Event::IndexPrice(proto::IndexPrice {
                        price: Some(dec(&m.index)?.into()),
                    }),
                ),
                n(
                    MarketDataKind::Funding,
                    &m.symbol,
                    ts,
                    None,
                    market_event::Event::Funding(proto::Funding {
                        rate: Some(dec(&m.funding_rate)?.into()),
                        interval_ns: None,
                        next_funding_time: Some(ms(m.next_funding_time)),
                    }),
                ),
            ]
        }
        Parsed::ForceOrder(f) => vec![n(
            MarketDataKind::Liquidations,
            &f.order.symbol,
            ms(f.order.trade_time),
            None,
            market_event::Event::Liquidation(proto::Liquidation {
                side: match f.order.side.as_str() {
                    "BUY" => proto::Side::Buy,
                    "SELL" => proto::Side::Sell,
                    _ => proto::Side::Unspecified,
                } as i32,
                price: Some(dec(&f.order.avg_price)?.into()),
                qty_base: Some(dec(&f.order.filled_qty)?.into()),
            }),
        )],
        Parsed::Depth(_) => {
            return Err(NormalizeError::Unsupported(
                "depthUpdate se normaliza con LocalBook".into(),
            ));
        }
    })
}

/// Trade agregado de la respuesta REST `aggTrades` (`/api/v3/aggTrades`,
/// `/fapi/v1/aggTrades`).
#[derive(Clone, Debug, Deserialize, PartialEq, Eq)]
pub struct RestAggTrade {
    #[serde(rename = "a")]
    pub agg_id: u64,
    #[serde(rename = "p")]
    pub price: String,
    #[serde(rename = "q")]
    pub qty: String,
    #[serde(rename = "T")]
    pub trade_time: i64,
    #[serde(rename = "m")]
    pub buyer_is_maker: bool,
}

/// Interpreta la respuesta REST `aggTrades`.
pub fn parse_rest_agg_trades(raw: &[u8]) -> Result<Vec<RestAggTrade>, NormalizeError> {
    serde_json::from_slice(raw).map_err(|e| NormalizeError::Json(e.to_string()))
}

/// Normaliza un trade de la respuesta REST `aggTrades`: el mismo evento que daría el
/// `aggTrade` del WS con ese id.
pub fn rest_agg_trade(
    market: Market,
    symbol: &str,
    t: &RestAggTrade,
) -> Result<Normalized, NormalizeError> {
    let ws = AggTrade {
        event_time: t.trade_time,
        symbol: symbol.to_ascii_uppercase(),
        agg_id: t.agg_id,
        price: t.price.clone(),
        qty: t.qty.clone(),
        trade_time: t.trade_time,
        buyer_is_maker: t.buyer_is_maker,
    };
    let mut out = normalize(market, &Parsed::AggTrade(ws), 0)?;
    Ok(out.remove(0))
}

/// Interpreta la respuesta REST del snapshot del libro (`/api/v3/depth`,
/// `/fapi/v1/depth`).
pub fn parse_depth_snapshot(raw: &[u8]) -> Result<DepthSnapshot, NormalizeError> {
    serde_json::from_slice(raw).map_err(|e| NormalizeError::Json(e.to_string()))
}

#[derive(Deserialize)]
struct OpenInterestResponse {
    #[serde(rename = "openInterest")]
    open_interest: String,
    symbol: String,
    time: i64,
}

/// Normaliza la respuesta de `/fapi/v1/openInterest`.
pub fn open_interest(raw: &[u8]) -> Result<Normalized, NormalizeError> {
    let r: OpenInterestResponse =
        serde_json::from_slice(raw).map_err(|e| NormalizeError::Json(e.to_string()))?;
    Ok(Normalized {
        source: source(Market::UsdM, &r.symbol, MarketDataKind::OpenInterest),
        ts_event: ms(r.time),
        source_seq: None,
        event: event(market_event::Event::OpenInterest(proto::OpenInterest {
            qty_base: Some(dec(&r.open_interest)?.into()),
        })),
    })
}
