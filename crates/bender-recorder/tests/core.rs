//! Núcleo del grabador con tiempo simulado: cortes, recuperación de trades, libro y
//! reinicio (criterios de #32).

mod support;

use std::collections::BTreeSet;

use bender_kernel::CoverageProjection;
use bender_recorder::core::{BookMode, Core, Input, Record, Startup};
use bender_store::{Store, WriterOptions, Zone};
use bender_types::proto::{
    self, CoverageState, GapReason, Provenance, RecoveryMethod, RecoveryResult,
};
use bender_venue_binance::Market;
use support::*;

const TRADES: &str = "BINANCE:PERP_LINEAR:BTCUSDT:trades";
const BOOK: &str = "BINANCE:PERP_LINEAR:BTCUSDT:book";

fn sim(book: BookMode) -> Sim {
    let core = Core::new(
        config(vec![instrument(Market::UsdM, "BTCUSDT", book)]),
        Startup::default(),
    );
    Sim::new(core, Market::UsdM, "BTCUSDT")
}

fn coverage(
    events: &[proto::Envelope],
    source: &str,
    from: i64,
    to: i64,
    as_of: i64,
) -> Vec<(i64, i64, CoverageState)> {
    let mut p = CoverageProjection::new();
    let mut sorted = events.to_vec();
    sorted.sort_by_key(|e| (e.ts_available, e.source_seq, e.ingest_seq));
    for e in &sorted {
        p.apply(e);
    }
    p.coverage(source, from, to, as_of)
        .into_iter()
        .map(|c| (c.from, c.to, c.state))
        .collect()
}

/// Graba trades 1..=100 en vivo, corta 60 s (trades 101..=700 no llegan), reconecta y
/// sigue con 701..=800.
fn cut_60s(sim: &mut Sim) -> (i64, i64) {
    let t0 = 1_761_580_800_000; // ms
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: t0 * MS - S,
    });
    for t in trades(1, 100, t0, 100) {
        sim.trade(t, true);
    }
    let last_seen = (t0 + 99 * 100) * MS + 5 * MS;
    let cut = last_seen + 500 * MS;
    sim.input(Input::Disconnected {
        market: Market::UsdM,
        ts: cut,
        reason: GapReason::Disconnected,
        detail: "conexión cerrada".into(),
    });
    // 600 trades durante el corte (uno cada 100 ms = 60 s).
    for t in trades(101, 700, t0 + 10_000, 100) {
        sim.trade(t, false);
    }
    let reconnect = (t0 + 70_000) * MS;
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: reconnect,
    });
    for t in trades(701, 800, t0 + 70_050, 100) {
        sim.trade(t, true);
    }
    (last_seen, reconnect)
}

#[test]
fn cut_of_60s_is_fully_recovered_without_duplicates() {
    let mut sim = sim(BookMode::Top);
    let (last_seen, reconnect) = cut_60s(&mut sim);
    let events = sim.of_source(TRADES);

    // El Gap original sigue en el log, con los bordes del corte.
    let gaps = gaps(&events);
    assert_eq!(gaps.len(), 1);
    assert_eq!(gaps[0].reason, GapReason::Disconnected as i32);
    assert_eq!((gaps[0].from_ts, gaps[0].to_ts), (last_seen, reconnect));

    // Todos los trades, una sola vez cada uno.
    let ids: Vec<u64> = events
        .iter()
        .filter(|e| is_trade(e))
        .map(|e| e.source_seq.unwrap())
        .collect();
    let unique: BTreeSet<u64> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "trades duplicados");
    assert_eq!(unique, (1..=800).collect());

    // Los del corte, como recuperados y disponibles después de reconectar.
    for e in events.iter().filter(|e| is_trade(e)) {
        let id = e.source_seq.unwrap();
        let expected = if (101..=700).contains(&id) {
            Provenance::Recovered
        } else {
            Provenance::Live
        };
        assert_eq!(e.provenance, expected as i32, "trade {id}");
        if expected == Provenance::Recovered {
            assert!(e.ts_available > reconnect);
        }
    }

    // GapRecovery full por ids.
    let rec = recoveries(&events);
    assert_eq!(rec.len(), 1);
    let (recovered_at, rec) = &rec[0];
    assert_eq!(rec.result, RecoveryResult::Full as i32);
    assert_eq!(rec.method, RecoveryMethod::RestById as i32);
    assert_eq!(rec.gap_id, gaps[0].gap_id);

    // Cobertura: antes de la recuperación, incomplete; después, recovered.
    let before = coverage(
        &events,
        TRADES,
        last_seen - S,
        reconnect + S,
        recovered_at - 1,
    );
    assert_eq!(before[1], (last_seen, reconnect, CoverageState::Incomplete));
    let after = coverage(&events, TRADES, last_seen - S, reconnect + S, *recovered_at);
    assert_eq!(after[1], (last_seen, reconnect, CoverageState::Recovered));

    // El resto de las fuentes de la conexión: Gap + GapRecovery none.
    for kind in [
        "top_of_book",
        "mark_price",
        "index_price",
        "funding",
        "liquidations",
    ] {
        let ev = sim.of_source(&format!("BINANCE:PERP_LINEAR:BTCUSDT:{kind}"));
        assert_eq!(gaps_of(&ev), 1, "{kind}");
        let r = recoveries(&ev);
        assert_eq!(r[0].1.method, RecoveryMethod::None as i32);
        assert_eq!(r[0].1.result, RecoveryResult::None as i32);
    }
    assert_per_source_order(&sim.records);
}

fn gaps_of(ev: &[proto::Envelope]) -> usize {
    gaps(ev).len()
}

#[test]
fn provider_returns_part_of_the_range() {
    let mut sim = sim(BookMode::Top);
    sim.rest_available_until = 400;
    let (last_seen, reconnect) = cut_60s(&mut sim);
    let events = sim.of_source(TRADES);
    let recovered: Vec<u64> = events
        .iter()
        .filter(|e| is_trade(e) && e.provenance == Provenance::Recovered as i32)
        .map(|e| e.source_seq.unwrap())
        .collect();
    assert_eq!(recovered, (101..=400).collect::<Vec<_>>());
    let rec = recoveries(&events);
    assert_eq!(rec.len(), 1);
    let (at, rec) = &rec[0];
    assert_eq!(rec.result, RecoveryResult::Partial as i32);
    // Recuperado hasta el último trade devuelto (id 400).
    let t400 = (1_761_580_800_000 + 10_000 + 299 * 100) * MS;
    assert_eq!(
        (rec.recovered_from, rec.recovered_to),
        (Some(last_seen), Some(t400 + 1))
    );
    // Cobertura mixta: recovered y después incomplete.
    let cov = coverage(&events, TRADES, last_seen - S, reconnect + S, *at);
    assert_eq!(
        cov,
        vec![
            (last_seen - S, last_seen, CoverageState::Complete),
            (last_seen, t400 + 1, CoverageState::Recovered),
            (t400 + 1, reconnect, CoverageState::Incomplete),
            (reconnect, reconnect + S, CoverageState::Complete),
        ]
    );
    assert_per_source_order(&sim.records);
}

#[test]
fn trade_id_jump_without_disconnect_is_a_gap_and_recovers() {
    let mut sim = sim(BookMode::Top);
    let t0 = 1_761_580_800_000;
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: t0 * MS,
    });
    for t in trades(1, 10, t0, 100) {
        sim.trade(t, true);
    }
    for t in trades(11, 15, t0 + 1_000, 100) {
        sim.trade(t, false);
    }
    for t in trades(16, 20, t0 + 1_500, 100) {
        sim.trade(t, true);
    }
    let events = sim.of_source(TRADES);
    let g = gaps(&events);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].reason, GapReason::SequenceBreak as i32);
    assert_eq!(recoveries(&events)[0].1.result, RecoveryResult::Full as i32);
    let ids: BTreeSet<u64> = events
        .iter()
        .filter(|e| is_trade(e))
        .map(|e| e.source_seq.unwrap())
        .collect();
    assert_eq!(ids, (1..=20).collect());
}

#[test]
fn book_sequence_break_resyncs_and_leaves_the_interval_incomplete() {
    let mut sim = sim(BookMode::L2);
    sim.snapshot = Some((1000, vec![(100, 1), (99, 2)], vec![(101, 1), (102, 2)]));
    let t0 = 1_761_580_800_000i64 * MS;
    let depth = |first: u64, last: u64, pu: u64, ms: i64, bid: i64| -> Vec<u8> {
        format!(
            r#"{{"stream":"btcusdt@depth@100ms","data":{{"e":"depthUpdate","E":{ms},"T":{ms},"s":"BTCUSDT","U":{first},"u":{last},"pu":{pu},"b":[["{bid}.00","1.000"]],"a":[]}}}}"#
        )
        .into_bytes()
    };
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: t0,
    });
    // El snapshot llega antes que los deltas (latencia 50 ms): los deltas se aplican.
    let mut u = 999;
    for i in 0..10 {
        let ts = t0 + 100 * MS * (i + 1);
        sim.input(Input::Message {
            market: Market::UsdM,
            ts,
            raw: depth(u + 1, u + 5, u, ts / MS, 90 + i),
        });
        u += 5;
    }
    let last_good = t0 + 100 * MS * 10;
    // Salto: pu no coincide con el u anterior.
    sim.snapshot = Some((u + 100, vec![(100, 3)], vec![(101, 3)]));
    let break_ts = last_good + 100 * MS;
    sim.input(Input::Message {
        market: Market::UsdM,
        ts: break_ts,
        raw: depth(u + 50, u + 60, u + 40, break_ts / MS, 80),
    });
    // El snapshot se pidió y llegó 50 ms después.
    let events = sim.of_source(BOOK);
    let g = gaps(&events);
    assert_eq!(g.len(), 1, "un hueco del libro");
    assert_eq!(g[0].reason, GapReason::SequenceBreak as i32);
    let resync = break_ts + 50 * MS;
    assert_eq!((g[0].from_ts, g[0].to_ts), (last_good, resync));
    let r = recoveries(&events);
    assert_eq!(r[0].1.method, RecoveryMethod::Resnapshot as i32);
    assert_eq!(r[0].1.result, RecoveryResult::None as i32);
    let cov = coverage(&events, BOOK, last_good - S, resync + S, resync + S);
    assert_eq!(cov[1], (last_good, resync, CoverageState::Incomplete));
    // Después del Gap: snapshot nuevo, y los deltas que enganchan se aplican.
    let snapshots = events
        .iter()
        .filter(|e| {
            matches!(
                &e.payload,
                Some(proto::envelope::Payload::Market(proto::MarketEvent {
                    event: Some(proto::market_event::Event::BookSnapshot(_))
                }))
            )
        })
        .count();
    assert_eq!(snapshots, 2);
    let ts = resync + 10 * MS;
    sim.input(Input::Message {
        market: Market::UsdM,
        ts,
        raw: depth(u + 61, u + 120, u + 60, ts / MS, 81),
    });
    assert!(matches!(
        &sim.of_source(BOOK).last().unwrap().payload,
        Some(proto::envelope::Payload::Market(proto::MarketEvent {
            event: Some(proto::market_event::Event::BookDelta(_))
        }))
    ));
    assert_per_source_order(&sim.records);
}

#[test]
fn stale_connection_requests_a_reconnect() {
    let mut sim = sim(BookMode::Top);
    let t0 = 1_761_580_800_000i64 * MS;
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: t0,
    });
    sim.tick(t0 + 5 * S);
    assert!(
        !sim.requests
            .iter()
            .any(|r| matches!(r, bender_recorder::core::Request::Reconnect { .. }))
    );
    sim.tick(t0 + 11 * S);
    sim.tick(t0 + 12 * S);
    let reconnects = sim
        .requests
        .iter()
        .filter(|r| matches!(r, bender_recorder::core::Request::Reconnect { reason, .. } if *reason == GapReason::Stale))
        .count();
    assert_eq!(reconnects, 1);
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bender-recorder-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn write(store: &mut Store, records: &[Record]) {
    let opts = WriterOptions::default();
    for r in records {
        match r {
            Record::Normalized { source, envelope } => store
                .append_envelope(
                    source,
                    bender_venue_binance::NORMALIZER_VERSION,
                    &opts,
                    envelope,
                )
                .unwrap(),
            Record::Raw { source, record } => store.append_raw(source, &opts, record).unwrap(),
        }
    }
}

#[test]
fn restart_after_shutdown_leaves_a_gap_and_recovers() {
    let dir = temp_dir("restart");
    let t0 = 1_761_580_800_000i64;
    let all = trades(1, 400, t0, 100);
    // Primera corrida: trades 1..=50 y latidos; se apaga.
    {
        let mut store = Store::open(&dir).unwrap();
        let core = Core::new(
            config(vec![instrument(Market::UsdM, "BTCUSDT", BookMode::Top)]),
            Startup::from_store(&store).unwrap(),
        );
        let mut sim = Sim::new(core, Market::UsdM, "BTCUSDT");
        sim.input(Input::Connected {
            market: Market::UsdM,
            ts: t0 * MS,
        });
        for t in &all[..50] {
            sim.trade(*t, true);
            sim.tick(t.ts_ms * MS + 10 * MS);
        }
        write(&mut store, &sim.records);
        store.close_all().unwrap();
    }
    let last_heartbeat = (t0 + 49 * 100) * MS + 10 * MS;
    // Segunda corrida, 30 s después.
    let mut store = Store::open(&dir).unwrap();
    let startup = Startup::from_store(&store).unwrap();
    assert_eq!(startup.last_alive(), Some(last_heartbeat));
    assert_eq!(startup.last_trade_id.get(TRADES), Some(&50));
    let core = Core::new(
        config(vec![instrument(Market::UsdM, "BTCUSDT", BookMode::Top)]),
        startup,
    );
    let mut sim = Sim::new(core, Market::UsdM, "BTCUSDT");
    sim.all_trades = all.clone();
    let start = last_heartbeat + 30 * S;
    sim.input(Input::Connected {
        market: Market::UsdM,
        ts: start,
    });
    sim.trade(
        Trade {
            id: 401,
            ts_ms: start / MS + 10,
            price: 100_000,
        },
        true,
    );
    let events = sim.of_source(TRADES);
    let g = gaps(&events);
    assert_eq!(g.len(), 1);
    assert_eq!(g[0].reason, GapReason::ProcessDown as i32);
    assert_eq!((g[0].from_ts, g[0].to_ts), (last_heartbeat, start));
    // Recuperación intentada y completa: 51..=400.
    assert!(sim.requests.iter().any(|r| matches!(
        r,
        bender_recorder::core::Request::AggTrades { from_id: 51, .. }
    )));
    let recovered = events
        .iter()
        .filter(|e| is_trade(e) && e.provenance == Provenance::Recovered as i32)
        .count();
    assert_eq!(recovered, 350);
    assert_eq!(recoveries(&events)[0].1.result, RecoveryResult::Full as i32);
    // Las fuentes sin datos previos (liquidaciones) no reciben un Gap de apagado.
    assert!(gaps(&sim.of_source("BINANCE:PERP_LINEAR:BTCUSDT:liquidations")).is_empty());
    // Se escribe a continuación de lo anterior sin chocar ingest_seq.
    write(&mut store, &sim.records);
    store.close_all().unwrap();
    assert!(store.last_ingest_seq(Zone::Normalized, TRADES).unwrap() > 50);
    let _ = std::fs::remove_dir_all(&dir);
}
