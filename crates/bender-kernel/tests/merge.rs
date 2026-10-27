//! Criterios de #23 sobre la fusión en orden K.

use bender_kernel::{Merge, MergeError, SourceOrder, SourceRankTable, compare};
use bender_types::proto::{self, source_id};
use proptest::prelude::*;

/// Evento mínimo para los tests (el núcleo funciona igual con `Envelope`).
#[derive(Clone, Debug, PartialEq, Eq)]
struct Ev {
    ts: i64,
    seq: Option<u64>,
    ingest: u64,
    /// Para identificar el evento en las aserciones.
    source: usize,
}

impl bender_kernel::Ordered for Ev {
    fn ts_available(&self) -> i64 {
        self.ts
    }
    fn source_seq(&self) -> Option<u64> {
        self.seq
    }
    fn ingest_seq(&self) -> u64 {
        self.ingest
    }
}

/// Fuentes posibles: reloj, usuario y varios instrumentos de varios venues (así hay
/// empates de `source_rank` entre fuentes distintas).
fn all_sources() -> Vec<proto::SourceId> {
    let market = |venue: proto::Venue, symbol: &str, kind: proto::MarketDataKind| proto::SourceId {
        kind: Some(source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: venue as i32,
                market_type: proto::MarketType::PerpLinear as i32,
                symbol: symbol.into(),
            }),
            data_kind: kind as i32,
        })),
    };
    vec![
        proto::SourceId {
            kind: Some(source_id::Kind::Clock(proto::ClockSource {})),
        },
        proto::SourceId {
            kind: Some(source_id::Kind::User(proto::UserSource {
                stream: "drawings".into(),
            })),
        },
        market(
            proto::Venue::Binance,
            "BTCUSDT",
            proto::MarketDataKind::Trades,
        ),
        market(
            proto::Venue::Binance,
            "BTCUSDT",
            proto::MarketDataKind::Book,
        ),
        market(
            proto::Venue::Binance,
            "ETHUSDT",
            proto::MarketDataKind::Trades,
        ),
        market(
            proto::Venue::Bybit,
            "BTCUSDT",
            proto::MarketDataKind::Trades,
        ),
        market(
            proto::Venue::Blofin,
            "BTC-USDT",
            proto::MarketDataKind::Trades,
        ),
        market(proto::Venue::Mexc, "BTCUSDT", proto::MarketDataKind::Trades),
    ]
}

/// Eventos de una fuente, ya ordenados por K dentro de ella: ts no decreciente,
/// `ingest_seq` creciente y `source_seq` creciente cuando existe.
fn source_events(source: usize) -> impl Strategy<Value = Vec<Ev>> {
    (
        prop::collection::vec((0i64..4, any::<bool>()), 0..40),
        any::<bool>(),
    )
        .prop_map(move |(steps, with_seq)| {
            let mut ts = 0;
            steps
                .into_iter()
                .enumerate()
                .map(|(i, (dt, seq_here))| {
                    ts += dt; // saltos chicos → muchos ts iguales entre fuentes
                    Ev {
                        ts,
                        seq: (with_seq && seq_here).then_some(i as u64 * 3),
                        ingest: i as u64 + 1,
                        source,
                    }
                })
                .collect()
        })
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(512))]

    /// Para cualquier conjunto de fuentes y cualquier orden de llegada de los streams, la
    /// fusión es idéntica al `sort` por K de la unión.
    #[test]
    fn merge_equals_sort_of_union(
        picked in prop::sample::subsequence((0..8usize).collect::<Vec<_>>(), 0..=8),
        seeds in prop::collection::vec(any::<u64>(), 8),
        events in (0..8usize).prop_flat_map(|_| {
            (source_events(0), source_events(1), source_events(2), source_events(3),
             source_events(4), source_events(5), source_events(6), source_events(7))
        }),
    ) {
        let table = SourceRankTable::current();
        let sources = all_sources();
        let orders: Vec<SourceOrder> =
            sources.iter().map(|s| table.source_order(s).unwrap()).collect();
        let mut per_source = [events.0, events.1, events.2, events.3, events.4, events.5, events.6, events.7];
        // Precondición de la fusión: cada stream ya viene ordenado por K dentro de su
        // fuente. Con `source_seq` ausente en algunos eventos, K puede diferir del orden
        // de ingesta.
        for (i, events) in per_source.iter_mut().enumerate() {
            events.sort_by(|a, b| compare(a, &orders[i], b, &orders[i]));
        }

        // Orden de llegada arbitrario: se permutan las fuentes elegidas.
        let mut arrival = picked.clone();
        arrival.sort_by_key(|i| seeds[*i]);

        let merged: Vec<Ev> = Merge::new(
            arrival.iter().map(|&i| (orders[i].clone(), per_source[i].clone().into_iter())),
        )
        .unwrap()
        .collect();

        let mut union: Vec<Ev> = picked.iter().flat_map(|&i| per_source[i].clone()).collect();
        union.sort_by(|a, b| compare(a, &orders[a.source], b, &orders[b.source]));
        prop_assert_eq!(merged, union);
    }
}

#[test]
fn identical_k_except_source_orders_by_source_rank() {
    let table = SourceRankTable::current();
    let sources = all_sources();
    // CLOCK(0), USER(1), BINANCE(2), BLOFIN(6) y MEXC(7) con exactamente el mismo
    // ts_available, source_seq e ingest_seq.
    let picked = [7usize, 6, 2, 1, 0];
    let streams = picked.iter().map(|&i| {
        let ev = Ev {
            ts: 1_000,
            seq: Some(5),
            ingest: 9,
            source: i,
        };
        (
            table.source_order(&sources[i]).unwrap(),
            vec![ev].into_iter(),
        )
    });
    let order: Vec<usize> = Merge::new(streams).unwrap().map(|e| e.source).collect();
    // CLOCK < USER < BINANCE < BLOFIN < MEXC.
    assert_eq!(order, vec![0, 1, 2, 6, 7]);
}

#[test]
fn same_rank_ties_break_by_source_key_and_absent_seq_goes_first() {
    let table = SourceRankTable::current();
    let sources = all_sources();
    let ev = |source, seq| Ev {
        ts: 10,
        seq,
        ingest: 1,
        source,
    };
    // Tres fuentes de BINANCE: book (sin source_seq), ETHUSDT y BTCUSDT trades.
    let streams = vec![
        (
            table.source_order(&sources[4]).unwrap(),
            vec![ev(4, Some(1))].into_iter(),
        ),
        (
            table.source_order(&sources[2]).unwrap(),
            vec![ev(2, Some(1))].into_iter(),
        ),
        (
            table.source_order(&sources[3]).unwrap(),
            vec![ev(3, None)].into_iter(),
        ),
    ];
    let order: Vec<usize> = Merge::new(streams).unwrap().map(|e| e.source).collect();
    // source_seq ausente primero; después, por clave: ...:BTCUSDT:trades < ...:ETHUSDT:trades.
    assert_eq!(order, vec![3, 2, 4]);
}

#[test]
fn duplicate_source_is_rejected() {
    let table = SourceRankTable::current();
    let s = table.source_order(&all_sources()[0]).unwrap();
    let streams = vec![
        (s.clone(), Vec::<Ev>::new().into_iter()),
        (s, Vec::<Ev>::new().into_iter()),
    ];
    assert!(matches!(
        Merge::new(streams),
        Err(MergeError::DuplicateSource(_))
    ));
}

#[test]
fn merges_envelopes() {
    let table = SourceRankTable::current();
    let sources = all_sources();
    let env = |ts, ingest, source: &proto::SourceId| proto::Envelope {
        ts_available: ts,
        ingest_seq: ingest,
        source: Some(source.clone()),
        ..Default::default()
    };
    let a = vec![env(1, 1, &sources[2]), env(3, 2, &sources[2])];
    let b = vec![env(2, 1, &sources[0]), env(3, 2, &sources[0])];
    let merged: Vec<(i64, i32)> = Merge::new([
        (table.source_order(&sources[2]).unwrap(), a.into_iter()),
        (table.source_order(&sources[0]).unwrap(), b.into_iter()),
    ])
    .unwrap()
    .map(|e| {
        let is_clock = matches!(
            e.source.as_ref().and_then(|s| s.kind.as_ref()),
            Some(source_id::Kind::Clock(_))
        );
        (e.ts_available, i32::from(is_clock))
    })
    .collect();
    assert_eq!(merged, vec![(1, 0), (2, 1), (3, 1), (3, 0)]);
}
