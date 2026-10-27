//! Criterios de #24: reloj, temporizadores, latidos sintéticos y cobertura.

use bender_kernel::{
    CoverageProjection, DEFAULT_GRID_NS, Fired, KernelClock, Ordered, SourceOrder,
    synthetic_heartbeats,
};
use bender_types::proto::{self, CoverageState, RecoveryResult, envelope};
use bender_types::{Ulid, source_key};

#[derive(Clone, Debug)]
struct Ev {
    ts: i64,
    ingest: u64,
}

impl Ordered for Ev {
    fn ts_available(&self) -> i64 {
        self.ts
    }
    fn source_seq(&self) -> Option<u64> {
        None
    }
    fn ingest_seq(&self) -> u64 {
        self.ingest
    }
}

fn src(rank: u32, key: &str) -> SourceOrder {
    SourceOrder {
        rank,
        key: key.into(),
    }
}

#[test]
fn timer_fires_exactly_once_even_if_several_instants_pass() {
    let mut clock = KernelClock::new();
    let s = src(2, "BINANCE:SPOT:BTCUSDT:trades");
    for at in [10, 20, 30, 40] {
        clock.schedule(at, format!("t{at}"));
    }
    let mut fired = Vec::new();
    clock
        .advance(&Ev { ts: 5, ingest: 1 }, &s, &mut fired)
        .unwrap();
    assert!(fired.is_empty());
    // Entre 5 y 35 pasaron 10, 20 y 30: cada uno se dispara una vez, en orden.
    clock
        .advance(&Ev { ts: 35, ingest: 2 }, &s, &mut fired)
        .unwrap();
    let tags: Vec<_> = fired.iter().map(|f| f.tag.as_str()).collect();
    assert_eq!(tags, ["t10", "t20", "t30"]);
    fired.clear();
    clock
        .advance(&Ev { ts: 39, ingest: 3 }, &s, &mut fired)
        .unwrap();
    assert!(fired.is_empty(), "no se vuelven a disparar");
    // Un evento exactamente en el instante lo dispara (ts_available ≥ T).
    clock
        .advance(&Ev { ts: 40, ingest: 4 }, &s, &mut fired)
        .unwrap();
    assert_eq!(fired.len(), 1);
    assert_eq!(clock.pending(), 0);
}

#[test]
fn same_instant_fires_in_creation_order_and_cancel_works() {
    let mut clock = KernelClock::new();
    let a = clock.schedule(100, 'a');
    let b = clock.schedule(100, 'b');
    let c = clock.schedule(50, 'c');
    assert_eq!(clock.cancel(100, b), Some('b'));
    let mut fired = Vec::new();
    clock
        .advance(&Ev { ts: 100, ingest: 1 }, &src(0, "CLOCK"), &mut fired)
        .unwrap();
    let got: Vec<_> = fired.iter().map(|f: &Fired<char>| (f.id, f.tag)).collect();
    assert_eq!(got, vec![(c, 'c'), (a, 'a')]);
}

/// Corrida reproducible: eventos de dos fuentes y latidos, con temporizadores que se
/// reprograman al dispararse (como el cierre de velas).
fn run_once() -> Vec<(i64, i64, String)> {
    let trades = src(2, "BINANCE:SPOT:BTCUSDT:trades");
    let clock_src = src(0, "CLOCK");
    let beats: Vec<(Ev, &SourceOrder)> = synthetic_heartbeats(0, 2_000_000_000, DEFAULT_GRID_NS)
        .map(|e| {
            (
                Ev {
                    ts: e.ts_available,
                    ingest: e.ingest_seq,
                },
                &clock_src,
            )
        })
        .collect();
    let trades_evs: Vec<(Ev, &SourceOrder)> = (0..37u64)
        .map(|i| {
            (
                Ev {
                    ts: (i as i64) * 53_000_000 + 7,
                    ingest: i + 1,
                },
                &trades,
            )
        })
        .collect();
    let mut all: Vec<_> = beats.into_iter().chain(trades_evs).collect();
    all.sort_by(|(a, sa), (b, sb)| bender_kernel::compare(a, sa, b, sb));

    let mut clock = KernelClock::new();
    clock.schedule(250_000_000, "cierre 250ms".to_string());
    clock.schedule(250_000_000, "otro en el mismo instante".to_string());
    let mut fired = Vec::new();
    let mut log = Vec::new();
    for (ev, s) in &all {
        clock.advance(ev, s, &mut fired).unwrap();
        for f in fired.drain(..) {
            log.push((ev.ts, f.at, f.tag.clone()));
            if f.tag.starts_with("cierre") && f.at < 1_500_000_000 {
                clock.schedule(f.at + 250_000_000, f.tag);
            }
        }
    }
    log
}

#[test]
fn same_events_same_fires_in_two_runs() {
    let first = run_once();
    let second = run_once();
    assert_eq!(first, second);
    assert_eq!(first.len(), 7, "{first:?}"); // 6 cierres (250..1500) + el otro
    // Cada cierre se dispara con el primer evento en o después de su instante.
    assert!(
        first
            .iter()
            .all(|(ts, at, _)| ts >= at && ts - at < DEFAULT_GRID_NS)
    );
}

#[test]
fn out_of_order_event_is_a_typed_error() {
    let mut clock: KernelClock<()> = KernelClock::new();
    let s = src(2, "BINANCE:SPOT:BTCUSDT:trades");
    let mut fired = Vec::new();
    clock
        .advance(&Ev { ts: 100, ingest: 2 }, &s, &mut fired)
        .unwrap();
    let err = clock
        .advance(&Ev { ts: 99, ingest: 3 }, &s, &mut fired)
        .unwrap_err();
    assert_eq!((err.current_ts, err.got_ts), (100, 99));
    // El mismo evento repetido tampoco avanza (K igual).
    assert!(
        clock
            .advance(&Ev { ts: 100, ingest: 2 }, &s, &mut fired)
            .is_err()
    );
    // Mismo ts, fuente de rango menor: también es anterior en K.
    assert!(
        clock
            .advance(&Ev { ts: 100, ingest: 9 }, &src(0, "CLOCK"), &mut fired)
            .is_err()
    );
    assert_eq!(clock.now(), Some(100), "el reloj no cambió");
    clock
        .advance(&Ev { ts: 100, ingest: 3 }, &s, &mut fired)
        .unwrap();
}

#[test]
fn synthetic_heartbeats_count_and_alignment() {
    let g = DEFAULT_GRID_NS;
    for (a, b) in [
        (0, g),
        (0, g + 1),
        (5 * g, 5 * g + 10 * g),
        (7 * g, 7 * g + 3 * g - 1),
        (-3 * g, 2 * g),
    ] {
        let beats: Vec<_> = synthetic_heartbeats(a, b, g).collect();
        let expected = ((b - a) as f64 / g as f64).ceil() as usize; // a está en la grilla
        assert_eq!(beats.len(), expected, "[{a}, {b})");
        for e in &beats {
            assert_eq!(e.ts_available.rem_euclid(g), 0);
            assert!(e.ts_available >= a && e.ts_available < b);
            assert_eq!(e.provenance, proto::Provenance::Synthetic as i32);
            assert!(matches!(e.payload, Some(envelope::Payload::Heartbeat(_))));
        }
    }
    // Con `a` fuera de la grilla, sólo se emiten los puntos de la grilla dentro del rango.
    let beats: Vec<i64> = synthetic_heartbeats(g / 2, 2 * g + 1, g)
        .map(|e| e.ts_available)
        .collect();
    assert_eq!(beats, vec![g, 2 * g]);
    // Partir el rango no cambia los latidos (mismo ingest_seq).
    let whole: Vec<_> = synthetic_heartbeats(0, 10 * g, g).collect();
    let split: Vec<_> = synthetic_heartbeats(0, 4 * g, g)
        .chain(synthetic_heartbeats(4 * g, 10 * g, g))
        .collect();
    assert_eq!(whole, split);
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

fn gap(id: u64, from: i64, to: i64, known_at: i64) -> proto::Envelope {
    proto::Envelope {
        ts_available: known_at,
        source: Some(trades_source()),
        payload: Some(envelope::Payload::Gap(proto::Gap {
            gap_id: Some(Ulid::from_parts(1, u128::from(id)).into()),
            source: Some(trades_source()),
            from_ts: from,
            to_ts: to,
            reason: proto::GapReason::Disconnected as i32,
            detail: None,
        })),
        ..Default::default()
    }
}

fn recovery(
    id: u64,
    part: Option<(i64, i64)>,
    result: RecoveryResult,
    known_at: i64,
) -> proto::Envelope {
    proto::Envelope {
        ts_available: known_at,
        source: Some(trades_source()),
        payload: Some(envelope::Payload::GapRecovery(proto::GapRecovery {
            gap_id: Some(Ulid::from_parts(1, u128::from(id)).into()),
            recovered_from: part.map(|p| p.0),
            recovered_to: part.map(|p| p.1),
            method: proto::RecoveryMethod::RestById as i32,
            result: result as i32,
        })),
        ..Default::default()
    }
}

fn states(cov: &[bender_kernel::Coverage]) -> Vec<(i64, i64, CoverageState)> {
    cov.iter().map(|c| (c.from, c.to, c.state)).collect()
}

#[test]
fn coverage_follows_adr_0011() {
    use CoverageState::{Complete, Incomplete, Recovered};
    let key = source_key(&trades_source());
    let mut p = CoverageProjection::new();
    // Hueco 1: [100, 200), detectado en 200; recuperado entero en 500.
    p.apply(&gap(1, 100, 200, 200));
    p.apply(&recovery(1, Some((100, 200)), RecoveryResult::Full, 500));
    // Hueco 2: [300, 400), detectado en 400; recuperado en parte [300, 350) en 600.
    p.apply(&gap(2, 300, 400, 400));
    p.apply(&recovery(2, Some((300, 350)), RecoveryResult::Partial, 600));
    // Hueco 3: [700, 800), sin recuperación.
    p.apply(&gap(3, 700, 800, 800));
    // Hueco 4: [900, 950), intento sin resultado.
    p.apply(&gap(4, 900, 950, 950));
    p.apply(&recovery(4, None, RecoveryResult::None, 1_000));

    let now = 10_000;
    assert_eq!(
        states(&p.coverage(&key, 0, 1_000, now)),
        vec![
            (0, 100, Complete),
            (100, 200, Recovered),
            (200, 300, Complete),
            (300, 350, Recovered),
            (350, 400, Incomplete),
            (400, 700, Complete),
            (700, 800, Incomplete),
            (800, 900, Complete),
            (900, 950, Incomplete),
            (950, 1_000, Complete),
        ]
    );

    // as_of anterior a la recuperación: el hueco se ve como se vivió.
    assert_eq!(
        states(&p.coverage(&key, 0, 300, 499)),
        vec![
            (0, 100, Complete),
            (100, 200, Incomplete),
            (200, 300, Complete)
        ]
    );
    // as_of anterior a la detección: todavía no se sabía del hueco.
    assert_eq!(
        states(&p.coverage(&key, 0, 300, 199)),
        vec![(0, 300, Complete)]
    );
    // Otra fuente: completa.
    assert_eq!(
        states(&p.coverage("BINANCE:SPOT:BTCUSDT:trades", 0, 1_000, now)),
        vec![(0, 1_000, Complete)]
    );
    // Rango que corta un hueco por la mitad.
    assert_eq!(
        states(&p.coverage(&key, 320, 380, now)),
        vec![(320, 350, Recovered), (350, 380, Incomplete)]
    );
}
