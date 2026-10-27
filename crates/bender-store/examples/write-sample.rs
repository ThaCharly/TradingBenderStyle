//! Escribe un store de ejemplo determinista: trades de BTCUSDT y ETHUSDT (perp lineal de
//! Binance) durante dos días UTC, en las zonas normalizada y cruda. Lo usa el test de
//! DuckDB (`tools/store-sql`) y sirve para probar las consultas de
//! `docs/operacion/consultas-duckdb.md`.
//!
//! Uso: `cargo run -p bender-store --example write-sample -- <carpeta>`

use bender_store::{RawRecord, Store, WriterOptions};
use bender_types::proto::{self, envelope, market_event};
use bender_types::{Decimal, source_key};

fn source(symbol: &str) -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: proto::Venue::Binance as i32,
                market_type: proto::MarketType::PerpLinear as i32,
                symbol: symbol.into(),
            }),
            data_kind: proto::MarketDataKind::Trades as i32,
        })),
    }
}

fn main() {
    let dir = std::env::args()
        .nth(1)
        .expect("uso: write-sample <carpeta>");
    let mut store = Store::open(&dir).expect("abrir store");
    // 2025-10-27 12:00 UTC; 1.000 trades cada 2 min → cruza la medianoche.
    let start = 1_761_566_400_000_000_000i64;
    for (symbol, base_price) in [("BTCUSDT", 8_800_000i64), ("ETHUSDT", 320_000)] {
        let src = source(symbol);
        let key = source_key(&src);
        let mut normalized = Vec::new();
        let mut raw = Vec::new();
        for i in 1..=1_000u64 {
            let ts = start + i as i64 * 120_000_000_000;
            let price = base_price + (i as i64 % 50) - 25;
            normalized.push(proto::Envelope {
                ts_event: ts - 3_000_000,
                ts_received: Some(ts),
                ts_available: ts,
                source: Some(src.clone()),
                source_seq: Some(5_000_000 + i),
                ingest_seq: i,
                provenance: proto::Provenance::Live as i32,
                payload: Some(envelope::Payload::Market(proto::MarketEvent {
                    event: Some(market_event::Event::Trade(proto::Trade {
                        price: Some(Decimal::new(price.into(), 2).unwrap().into()),
                        qty_base: Some(Decimal::new(i128::from(i % 7) + 1, 3).unwrap().into()),
                        aggressor_side: if i % 2 == 0 {
                            proto::Side::Buy
                        } else {
                            proto::Side::Sell
                        } as i32,
                        venue_trade_id: Some(format!("{}", 5_000_000 + i)),
                    })),
                })),
            });
            raw.push(RawRecord {
                ts_received: ts,
                ingest_seq: i,
                payload: format!(
                    r#"{{"e":"trade","s":"{symbol}","t":{},"p":"{}","T":{}}}"#,
                    5_000_000 + i,
                    Decimal::new(price.into(), 2).unwrap(),
                    (ts - 3_000_000) / 1_000_000
                )
                .into_bytes(),
            });
        }
        let mut w = store.normalized_writer(&key, "binance-norm-1", WriterOptions::default());
        for e in &normalized {
            w.append_envelope(e).expect("escribir");
        }
        w.close().expect("cerrar");
        let mut w = store.raw_writer(&key, WriterOptions::default());
        for r in &raw {
            w.append_raw(r).expect("escribir");
        }
        w.close().expect("cerrar");
    }
    println!("{} segmentos en {dir}", store.segments().len());
}
