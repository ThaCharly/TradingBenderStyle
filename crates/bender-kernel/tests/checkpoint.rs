//! Criterios de #28: checkpoints canónicos y manifiestos de corrida.

use std::cell::RefCell;
use std::collections::BTreeMap;

use bender_kernel::{
    AvailabilityDelays, CheckpointSource, CompositeDefinition, KernelConfig, KernelState, Merge,
    PriceRule, RunManifest, SourceRankTable, Timeframe, checkpoint_day_start, restore,
    synthetic_heartbeats, user_envelope,
};
use bender_types::proto::{self, envelope, market_event, user_event};
use bender_types::{Decimal, NS_PER_DAY, days_from_civil};

const S: i64 = 1_000_000_000;
const BINANCE: &str = "BINANCE:PERP_LINEAR:BTCUSDT:trades";
const BYBIT: &str = "BYBIT:PERP_LINEAR:BTCUSDT:trades";

fn start() -> i64 {
    days_from_civil(2026, 9, 1) * NS_PER_DAY
}

fn market_source(venue: proto::Venue) -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: venue as i32,
                market_type: proto::MarketType::PerpLinear as i32,
                symbol: "BTCUSDT".into(),
            }),
            data_kind: proto::MarketDataKind::Trades as i32,
        })),
    }
}

fn trade(source: &proto::SourceId, seq: u64, ts: i64, price: i64, qty: i64) -> proto::Envelope {
    proto::Envelope {
        ts_event: ts - 3_000_000,
        ts_received: Some(ts),
        ts_available: ts,
        source: Some(source.clone()),
        source_seq: Some(seq),
        ingest_seq: seq,
        provenance: proto::Provenance::Live as i32,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(market_event::Event::Trade(proto::Trade {
                price: Some(Decimal::new(price.into(), 2).unwrap().into()),
                qty_base: Some(Decimal::new(qty.into(), 3).unwrap().into()),
                aggressor_side: if seq.is_multiple_of(2) {
                    proto::Side::Buy
                } else {
                    proto::Side::Sell
                } as i32,
                venue_trade_id: None,
            })),
        })),
    }
}

/// 30 días sintéticos: trades de dos venues, latidos cada 30 s y algunos dibujos, en
/// orden K.
fn thirty_days() -> Vec<proto::Envelope> {
    let table = SourceRankTable::current();
    let binance = market_source(proto::Venue::Binance);
    let bybit = market_source(proto::Venue::Bybit);
    let mut rng = 42u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let end = start() + 30 * NS_PER_DAY;
    let mut streams: Vec<Vec<proto::Envelope>> = Vec::new();
    for src in [&binance, &bybit] {
        let mut ts = start();
        let mut seq = 0;
        let mut price = 6_500_000i64;
        let mut v = Vec::new();
        while ts < end {
            ts += (next() % 120) as i64 * S + 1;
            seq += 1;
            price += (next() % 41) as i64 - 20;
            v.push(trade(src, seq, ts, price, (next() % 900 + 1) as i64));
        }
        streams.push(v);
    }
    streams.push(synthetic_heartbeats(start(), end, 30 * S).collect());
    let drawings: Vec<_> = (0..60u64)
        .map(|i| {
            let ev = proto::UserEvent {
                author: None,
                event: Some(user_event::Event::DrawingUpserted(proto::DrawingUpserted {
                    drawing_id: format!("d{}", i % 7),
                    payload: Some(proto::OpaquePayload {
                        version: 1,
                        data: vec![i as u8],
                    }),
                })),
            };
            user_envelope(
                "drawings",
                i + 1,
                None,
                start() + i as i64 * 11 * 3_600 * S + 17,
                ev,
            )
        })
        .collect();
    streams.push(drawings);
    let ordered = streams.into_iter().map(|v| {
        let src = v[0].source.clone().unwrap();
        (table.source_order(&src).unwrap(), v.into_iter())
    });
    Merge::new(ordered).unwrap().collect()
}

fn config() -> KernelConfig {
    KernelConfig {
        composites: vec![CompositeDefinition {
            id: "BTC/USD perps".into(),
            version: 1,
            members: vec![BINANCE.into(), BYBIT.into()],
            price_rule: PriceRule::Vwap,
        }],
        ..KernelConfig::default()
    }
}

/// Checkpoints guardados en memoria, con posibilidad de corromperlos.
#[derive(Default)]
struct MemCheckpoints(RefCell<BTreeMap<i64, Vec<u8>>>);

impl CheckpointSource for MemCheckpoints {
    fn checkpoint(&self, day_start: i64) -> Option<Vec<u8>> {
        self.0.borrow().get(&day_start).cloned()
    }
}

/// Corre todo, guardando un checkpoint al inicio de cada día (antes del primer evento del
/// día).
fn run_with_daily_checkpoints(events: &[proto::Envelope]) -> (KernelState, MemCheckpoints) {
    let cps = MemCheckpoints::default();
    let mut state = KernelState::new(config());
    let mut out = Vec::new();
    let mut day = None;
    for e in events {
        let d = checkpoint_day_start(e.ts_available);
        if day != Some(d) {
            if day.is_some() {
                cps.0.borrow_mut().insert(d, state.checkpoint());
            }
            day = Some(d);
        }
        state.apply(e, &mut out).unwrap();
    }
    (state, cps)
}

fn events_between(
    all: &[proto::Envelope],
) -> impl Fn(Option<i64>, i64) -> Vec<proto::Envelope> + '_ {
    move |from, to| {
        all.iter()
            .filter(|e| from.is_none_or(|f| e.ts_available >= f) && e.ts_available <= to)
            .cloned()
            .collect()
    }
}

#[test]
fn state_from_checkpoint_equals_state_from_scratch() {
    let events = thirty_days();
    assert!(events.len() > 100_000, "{}", events.len());
    let (_, cps) = run_with_daily_checkpoints(&events);
    assert!(cps.0.borrow().len() >= 29);

    let mut rng = 0x7357u64;
    for _ in 0..4 {
        rng = rng
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        let t = start() + (rng >> 11) as i64 % (30 * NS_PER_DAY);
        let fast = restore(&config(), t, &cps, &events_between(&events), 40).unwrap();
        assert!(fast.from_checkpoint.is_some(), "usa un checkpoint");
        let slow = restore(
            &config(),
            t,
            &MemCheckpoints::default(),
            &events_between(&events),
            40,
        )
        .unwrap();
        assert_eq!(slow.from_checkpoint, None);
        assert_eq!(fast.state.state_hash(), slow.state.state_hash(), "T = {t}");
        assert_eq!(fast.state.output_hash(), slow.state.output_hash());
        assert!(fast.state.events_applied() > 0);
    }
}

#[test]
fn two_serializations_of_the_same_state_are_identical() {
    let events = thirty_days();
    let mut a = KernelState::new(config());
    let mut b = KernelState::new(config());
    let mut out = Vec::new();
    for e in &events[..20_000] {
        a.apply(e, &mut out).unwrap();
        b.apply(e, &mut out).unwrap();
    }
    assert_eq!(a.checkpoint(), b.checkpoint());
    // Y releer un checkpoint y volver a serializarlo da los mismos bytes.
    let back = KernelState::from_checkpoint(&a.checkpoint()).unwrap();
    assert_eq!(back.checkpoint(), a.checkpoint());
    assert_eq!(back, a);
}

#[test]
fn changing_any_config_parameter_changes_the_manifest_hash() {
    let base = config();
    let state = KernelState::new(base.clone());
    let manifest = |c: &KernelConfig| {
        let segs = BTreeMap::from([(
            "normalized/x/2026-09-01/1-abc.parquet".to_string(),
            "abc".to_string(),
        )]);
        RunManifest::new(segs, "0123abcd", c, &state).hash()
    };
    let h0 = manifest(&base);
    let variants: Vec<(&str, KernelConfig)> = vec![
        (
            "L velas",
            KernelConfig {
                bars: bender_kernel::BarConfig {
                    lateness_ns: 3 * S,
                    ..base.bars
                },
                ..base.clone()
            },
        ),
        (
            "horizonte",
            KernelConfig {
                bars: bender_kernel::BarConfig {
                    revision_horizon_ns: 3_600 * S,
                    ..base.bars
                },
                ..base.clone()
            },
        ),
        (
            "M",
            KernelConfig {
                composite: bender_kernel::CompositeConfig {
                    max_wait_ns: 6 * S,
                    ..base.composite
                },
                ..base.clone()
            },
        ),
        (
            "L compuestas",
            KernelConfig {
                composite: bender_kernel::CompositeConfig {
                    lateness_ns: 1,
                    ..base.composite
                },
                ..base.clone()
            },
        ),
        (
            "G",
            KernelConfig {
                heartbeat_grid_ns: 200_000_000,
                ..base.clone()
            },
        ),
        (
            "δ_trade",
            KernelConfig {
                delays: AvailabilityDelays {
                    trade_ns: 1,
                    ..base.delays
                },
                ..base.clone()
            },
        ),
        (
            "δ_bar",
            KernelConfig {
                delays: AvailabilityDelays {
                    bar_ns: 1,
                    ..base.delays
                },
                ..base.clone()
            },
        ),
        (
            "δ_stat",
            KernelConfig {
                delays: AvailabilityDelays {
                    stat_ns: 1,
                    ..base.delays
                },
                ..base.clone()
            },
        ),
        (
            "versión source_rank",
            KernelConfig {
                source_rank_version: 2,
                ..base.clone()
            },
        ),
        (
            "hash source_rank",
            KernelConfig {
                source_rank_hash: "otro".into(),
                ..base.clone()
            },
        ),
        (
            "temporalidades",
            KernelConfig {
                timeframes: vec![Timeframe::Seconds(60)],
                ..base.clone()
            },
        ),
        ("versión de una definición", {
            let mut c = base.clone();
            c.composites[0].version = 2;
            c
        }),
        ("regla de precio", {
            let mut c = base.clone();
            c.composites[0].price_rule = PriceRule::Reference(BINANCE.into());
            c
        }),
        (
            "versiones de payload",
            KernelConfig {
                known_versions: bender_kernel::KnownVersions {
                    drawing: 2,
                    ..base.known_versions
                },
                ..base.clone()
            },
        ),
    ];
    for (what, c) in variants {
        assert_ne!(manifest(&c), h0, "{what} no cambió el hash");
    }
    // También los otros campos del manifiesto.
    let other_commit = RunManifest::new(BTreeMap::new(), "ffff", &base, &state).hash();
    assert_ne!(
        other_commit,
        RunManifest::new(BTreeMap::new(), "0123abcd", &base, &state).hash()
    );
}

#[test]
fn corrupt_checkpoint_is_detected_and_rebuilt_from_the_previous_one() {
    let events = thirty_days();
    let (_, cps) = run_with_daily_checkpoints(&events);
    let t = start() + 20 * NS_PER_DAY + 5 * 3_600 * S;
    let day = checkpoint_day_start(t);
    // Se corrompe un byte del checkpoint del día de T.
    {
        let mut map = cps.0.borrow_mut();
        let cp = map.get_mut(&day).unwrap();
        let mid = cp.len() / 2;
        cp[mid] ^= 0x40;
        assert!(
            KernelState::from_checkpoint(cp).is_err(),
            "se detecta al leer"
        );
    }
    let restored = restore(&config(), t, &cps, &events_between(&events), 40).unwrap();
    assert_eq!(restored.corrupt, vec![day]);
    assert_eq!(
        restored.from_checkpoint,
        Some(day - NS_PER_DAY),
        "usa el del día anterior"
    );
    let scratch = restore(
        &config(),
        t,
        &MemCheckpoints::default(),
        &events_between(&events),
        40,
    )
    .unwrap();
    assert_eq!(restored.state.state_hash(), scratch.state.state_hash());
}
