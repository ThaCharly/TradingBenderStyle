//! Criterios de #22 sobre la API pública del store.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use bender_store::{RawRecord, Store, StoreError, WriterOptions, Zone};
use bender_types::proto::{self, envelope, market_event};
use bender_types::{Decimal, source_key};
use proptest::prelude::*;

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn temp_root(tag: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("bender-store-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn trades_source() -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: proto::Venue::Binance as i32,
                market_type: proto::MarketType::PerpLinear as i32,
                symbol: "BTCUSDT".into(),
            }),
            data_kind: proto::MarketDataKind::Trades as i32,
        })),
    }
}

/// Envelope de trade determinista a partir de un número.
fn trade(seq: u64, ts_available: i64, price_cents: i64) -> proto::Envelope {
    proto::Envelope {
        ts_event: ts_available - 2_000_000,
        ts_received: Some(ts_available),
        ts_available,
        source: Some(trades_source()),
        source_seq: Some(1_000_000 + seq),
        ingest_seq: seq,
        provenance: proto::Provenance::Live as i32,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(market_event::Event::Trade(proto::Trade {
                price: Some(Decimal::new(price_cents.into(), 2).unwrap().into()),
                qty_base: Some(Decimal::new(i128::from(seq % 1000) + 1, 3).unwrap().into()),
                aggressor_side: proto::Side::Buy as i32,
                venue_trade_id: Some(format!("{}", 1_000_000 + seq)),
            })),
        })),
    }
}

fn write_all(root: &PathBuf, events: &[proto::Envelope], close_every: usize) {
    let key = source_key(&trades_source());
    let mut store = Store::open(root).unwrap();
    let mut w = store.normalized_writer(&key, "binance-norm-1", WriterOptions::default());
    for (i, e) in events.iter().enumerate() {
        w.append_envelope(e).unwrap();
        if close_every > 0 && (i + 1) % close_every == 0 {
            w.close().unwrap();
        }
    }
    w.close().unwrap();
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    /// Escribir N eventos, cerrar, releer → mismos eventos en el mismo orden. Incluye
    /// cortes de segmento arbitrarios y cambios de día.
    #[test]
    fn write_close_read_roundtrip(
        steps in prop::collection::vec((1i64..7_200_000_000_000, 1i64..10_000_000), 1..300),
        close_every in 0usize..50,
    ) {
        let mut ts = 1_761_600_000_000_000_000i64;
        let events: Vec<_> = steps
            .iter()
            .enumerate()
            .map(|(i, (dt, price))| {
                ts += dt;
                trade(i as u64 + 1, ts, *price)
            })
            .collect();
        let root = temp_root("prop");
        write_all(&root, &events, close_every);
        let store = Store::open(&root).unwrap();
        let back = store
            .read_normalized(&source_key(&trades_source()), i64::MIN, i64::MAX)
            .unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        prop_assert_eq!(back, events);
    }
}

#[test]
fn reads_by_range_in_ingest_order() {
    let root = temp_root("range");
    let base = 1_761_600_000_000_000_000i64;
    let events: Vec<_> = (1..=100)
        .map(|i| trade(i, base + i as i64 * 1_000, 100))
        .collect();
    write_all(&root, &events, 30);
    let store = Store::open(&root).unwrap();
    let key = source_key(&trades_source());
    let got: Vec<u64> = store
        .read_normalized(&key, base + 20_000, base + 41_000)
        .unwrap()
        .iter()
        .map(|e| e.ingest_seq)
        .collect();
    assert_eq!(got, (20..=40).collect::<Vec<_>>());
    std::fs::remove_dir_all(&root).unwrap();
}

/// El hash de contenido no depende de la máquina ni del proceso: la misma secuencia da
/// el mismo hash en dos stores distintos y coincide con el valor fijado acá (que la CI
/// verifica en otra máquina).
#[test]
fn content_hash_is_reproducible() {
    let base = 1_761_600_000_000_000_000i64;
    let events: Vec<_> = (1..=1_000)
        .map(|i| trade(i, base + i as i64 * 250_000, 8_800_000 + i as i64))
        .collect();
    let hashes: Vec<String> = (0..2)
        .map(|_| {
            let root = temp_root("hash");
            write_all(&root, &events, 0);
            let store = Store::open(&root).unwrap();
            let hash = store.segments()[0].content_hash.clone();
            std::fs::remove_dir_all(&root).unwrap();
            hash
        })
        .collect();
    assert_eq!(hashes[0], hashes[1]);
    assert_eq!(
        hashes[0], GOLDEN_HASH,
        "cambió la serialización canónica: si es a propósito, es un cambio de formato"
    );
}

const GOLDEN_HASH: &str = "6d0f6ee8dded68a08d00a443086143070367415cf48134a57fff0eec154c69ed";

#[test]
fn modified_byte_is_detected_on_read() {
    let root = temp_root("tamper");
    let base = 1_761_600_000_000_000_000i64;
    let events: Vec<_> = (1..=200)
        .map(|i| trade(i, base + i as i64, 100 + i as i64))
        .collect();
    write_all(&root, &events, 0);
    let store = Store::open(&root).unwrap();
    let path = root.join(&store.segments()[0].path);
    let mut bytes = std::fs::read(&path).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x01;
    std::fs::write(&path, bytes).unwrap();

    let store = Store::open(&root).unwrap();
    let err = store
        .read_normalized(&source_key(&trades_source()), i64::MIN, i64::MAX)
        .unwrap_err();
    assert!(matches!(err, StoreError::Integrity { .. }), "{err}");
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn raw_zone_roundtrip_and_partitioning() {
    let root = temp_root("raw");
    let key = source_key(&trades_source());
    let day = 86_400_000_000_000i64;
    let base = 1_761_609_600_000_000_000i64 - 3; // 3 ns antes de medianoche UTC
    let records: Vec<_> = (1..=6)
        .map(|i| RawRecord {
            ts_received: base + i as i64,
            ingest_seq: i,
            payload: format!("{{\"e\":\"trade\",\"t\":{i}}}").into_bytes(),
        })
        .collect();
    {
        let mut store = Store::open(&root).unwrap();
        let mut w = store.raw_writer(&key, WriterOptions::default());
        for r in &records {
            w.append_raw(r).unwrap();
        }
        w.close().unwrap();
        // ingest_seq repetido: rechazado.
        let mut w = store.raw_writer(&key, WriterOptions::default());
        assert!(matches!(
            w.append_raw(&records[0]),
            Err(StoreError::OutOfOrder { .. })
        ));
    }
    let store = Store::open(&root).unwrap();
    let segments = store.segments();
    assert_eq!(segments.len(), 2, "un segmento por día UTC");
    assert!(segments.iter().all(|s| s.zone == Zone::Raw));
    assert_eq!(segments[0].date, "2025-10-27");
    assert_eq!(segments[1].date, "2025-10-28");
    assert!(
        segments[0]
            .path
            .starts_with("raw/BINANCE~PERP_LINEAR~BTCUSDT~trades/2025-10-27/")
    );
    assert_eq!(store.read_raw(&key, base, base + day).unwrap(), records);
    std::fs::remove_dir_all(&root).unwrap();
}

#[test]
fn checkpoints_are_stored_by_name_and_day() {
    let root = temp_root("checkpoint");
    let store = Store::open(&root).unwrap();
    let day = 1_793_145_600_000_000_000i64; // 2026-10-28 00:00 UTC
    assert_eq!(store.get_checkpoint("kernel", day).unwrap(), None);
    let path = store.put_checkpoint("kernel", day, b"estado").unwrap();
    assert!(path.ends_with("checkpoints/kernel/2026-10-28.bin"));
    store
        .put_checkpoint("kernel", day, b"estado nuevo")
        .unwrap();
    assert_eq!(
        store.get_checkpoint("kernel", day).unwrap().as_deref(),
        Some(&b"estado nuevo"[..])
    );
    assert_eq!(store.get_checkpoint("otro", day).unwrap(), None);
    // Los checkpoints no son segmentos: Store::open no los toca.
    let store = Store::open(&root).unwrap();
    assert!(store.get_checkpoint("kernel", day).unwrap().is_some());
    std::fs::remove_dir_all(&root).unwrap();
}
