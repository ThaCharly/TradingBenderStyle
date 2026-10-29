//! Representación de texto estable de la salida del normalizador, para los fixtures
//! (`tests/fixtures/*/<nombre>.esperado`) y para revisar capturas a mano.

use bender_types::proto::{self, market_event};
use bender_types::{Decimal, source_key};

use crate::Normalized;

fn dec(d: &Option<proto::Decimal>) -> String {
    match (*d).map(Decimal::try_from) {
        Some(Ok(d)) => d.to_string(),
        Some(Err(e)) => format!("<decimal inválido: {e}>"),
        None => "-".into(),
    }
}

fn side(v: i32) -> &'static str {
    proto::Side::try_from(v).map_or("?", |s| match s {
        proto::Side::Buy => "BUY",
        proto::Side::Sell => "SELL",
        proto::Side::Unspecified => "-",
    })
}

fn book_side(v: i32) -> &'static str {
    proto::BookSide::try_from(v).map_or("?", |s| match s {
        proto::BookSide::Bid => "bid",
        proto::BookSide::Ask => "ask",
        proto::BookSide::Unspecified => "-",
    })
}

fn levels(levels: &[proto::BookLevel]) -> String {
    levels
        .iter()
        .map(|l| format!("{}x{}", dec(&l.price), dec(&l.qty_base)))
        .collect::<Vec<_>>()
        .join(" ")
}

/// Una línea por evento: fuente, instante, secuencia y contenido.
pub fn event(
    source: &proto::SourceId,
    ts_event: i64,
    seq: Option<u64>,
    e: &proto::MarketEvent,
) -> String {
    let head = format!(
        "{} ts_event={ts_event} seq={}",
        source_key(source),
        seq.map_or("-".into(), |s| s.to_string())
    );
    let body = match &e.event {
        Some(market_event::Event::Trade(t)) => format!(
            "trade price={} qty={} aggressor={} id={}",
            dec(&t.price),
            dec(&t.qty_base),
            side(t.aggressor_side),
            t.venue_trade_id.as_deref().unwrap_or("-")
        ),
        Some(market_event::Event::TopOfBook(t)) => format!(
            "top bid={}x{} ask={}x{}",
            dec(&t.bid),
            dec(&t.bid_qty),
            dec(&t.ask),
            dec(&t.ask_qty)
        ),
        Some(market_event::Event::BookSnapshot(b)) => format!(
            "snapshot venue_seq={} bids=[{}] asks=[{}]",
            b.venue_seq.map_or("-".into(), |s| s.to_string()),
            levels(&b.bids),
            levels(&b.asks)
        ),
        Some(market_event::Event::BookDelta(b)) => format!(
            "delta venue_seq={} [{}]",
            b.venue_seq.map_or("-".into(), |s| s.to_string()),
            b.changes
                .iter()
                .map(|c| format!(
                    "{}:{}x{}",
                    book_side(c.side),
                    dec(&c.price),
                    dec(&c.qty_base)
                ))
                .collect::<Vec<_>>()
                .join(" ")
        ),
        Some(market_event::Event::MarkPrice(m)) => format!("mark price={}", dec(&m.price)),
        Some(market_event::Event::IndexPrice(m)) => format!("index price={}", dec(&m.price)),
        Some(market_event::Event::Funding(f)) => format!(
            "funding rate={} next={}",
            dec(&f.rate),
            f.next_funding_time.map_or("-".into(), |t| t.to_string())
        ),
        Some(market_event::Event::Liquidation(l)) => format!(
            "liquidation side={} price={} qty={}",
            side(l.side),
            dec(&l.price),
            dec(&l.qty_base)
        ),
        Some(market_event::Event::OpenInterest(o)) => {
            format!("open_interest qty={}", dec(&o.qty_base))
        }
        other => format!("{other:?}"),
    };
    format!("{head} {body}")
}

/// Línea de un [`Normalized`].
pub fn normalized(n: &Normalized) -> String {
    event(&n.source, n.ts_event, n.source_seq, &n.event)
}

/// Top-N con el que se renderizan los fixtures de libro.
pub const FIXTURE_TOP_N: usize = 20;

/// Mercado y símbolo de un fixture según su nombre: `<spot|usdm>_<tipo>[_<SÍMBOLO>].json`
/// (sin símbolo: `BTCUSDT`, el de los mensajes del WS sale del propio mensaje).
pub fn fixture_target(stem: &str) -> Option<(crate::Market, String)> {
    let market = if stem.starts_with("spot_") {
        crate::Market::Spot
    } else if stem.starts_with("usdm_") {
        crate::Market::UsdM
    } else {
        return None;
    };
    let symbol = stem
        .rsplit('_')
        .next()
        .filter(|s| {
            s.chars()
                .all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        })
        .unwrap_or("BTCUSDT")
        .to_string();
    Some((market, symbol))
}

/// Salida esperada de un fixture crudo, según su tipo (por el nombre):
///
/// - `*depth_snapshot*`: snapshot REST → `BookSnapshot` del top-N;
/// - `*rest_aggtrades*`: respuesta REST `aggTrades` → trades;
/// - `*open_interest*`: respuesta REST → `OpenInterest`;
/// - `*depth*` (delta del WS): se aplica sobre `snapshot` (el snapshot del mismo mercado)
///   → `BookSnapshot` + `BookDelta`;
/// - el resto: mensaje del WS → eventos (con `ts_received = 0`).
pub fn fixture(
    stem: &str,
    raw: &[u8],
    snapshot: Option<&[u8]>,
) -> Result<String, crate::NormalizeError> {
    use crate::NormalizeError;
    use crate::book::{DiffOutcome, LocalBook, SnapshotOutcome};
    let (market, symbol) = fixture_target(stem)
        .ok_or_else(|| NormalizeError::Unsupported(format!("nombre de fixture: {stem}")))?;
    let mut lines = Vec::new();
    let book_source = crate::source(market, &symbol, proto::MarketDataKind::Book);
    let synced = |raw: &[u8], lines: &mut Vec<String>| -> Result<LocalBook, NormalizeError> {
        let snap = crate::parse_depth_snapshot(raw)?;
        let mut book = LocalBook::new(market, FIXTURE_TOP_N);
        if let SnapshotOutcome::Synced { snapshot, .. } = book.on_snapshot(&snap)? {
            lines.push(event(
                &book_source,
                snap.event_time.map_or(0, |t| t * 1_000_000),
                Some(snap.last_update_id),
                &proto::MarketEvent {
                    event: Some(market_event::Event::BookSnapshot(snapshot)),
                },
            ));
        }
        Ok(book)
    };
    if stem.contains("depth_snapshot") {
        synced(raw, &mut lines)?;
    } else if stem.contains("rest_aggtrades") {
        for t in crate::parse_rest_agg_trades(raw)? {
            lines.push(normalized(&crate::rest_agg_trade(market, &symbol, &t)?));
        }
    } else if stem.contains("open_interest") {
        lines.push(normalized(&crate::open_interest(raw)?));
    } else if stem.contains("depth") {
        let snapshot =
            snapshot.ok_or_else(|| NormalizeError::Unsupported("delta sin snapshot".into()))?;
        let mut book = synced(snapshot, &mut lines)?;
        let crate::Parsed::Depth(d) = crate::parse(raw)? else {
            return Err(NormalizeError::Unsupported("no es un depthUpdate".into()));
        };
        match book.on_diff(&d)? {
            DiffOutcome::Applied(Some(delta)) => lines.push(event(
                &book_source,
                d.event_time * 1_000_000,
                Some(d.final_update_id),
                &proto::MarketEvent {
                    event: Some(market_event::Event::BookDelta(delta)),
                },
            )),
            other => lines.push(format!("{other:?}")),
        }
    } else {
        for n in crate::normalize(market, &crate::parse(raw)?, 0)? {
            lines.push(normalized(&n));
        }
    }
    Ok(lines.join("\n") + "\n")
}
