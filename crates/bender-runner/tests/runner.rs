//! Criterios de #29: un solo Runner con modos live, replay, backtest y dataset.

use std::sync::mpsc;

use bender_kernel::{
    KernelConfig, KernelOutput, Merge, SourceRankTable, Timeframe, synthetic_heartbeats,
};
use bender_runner::{
    Consumer, DatasetRow, FutureWindow, Labeler, LiveSource, Pace, Pacer, PastView, Replay,
    StoreSource, backtest, dataset, live, write_dataset_parquet,
};
use bender_store::{Store, WriterOptions};
use bender_types::proto::{self, envelope, market_event};
use bender_types::{Decimal, NS_PER_DAY, days_from_civil, source_key};

const S: i64 = 1_000_000_000;

fn start() -> i64 {
    days_from_civil(2026, 10, 1) * NS_PER_DAY
}

fn market(venue: proto::Venue, symbol: &str) -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: venue as i32,
                market_type: proto::MarketType::PerpLinear as i32,
                symbol: symbol.into(),
            }),
            data_kind: proto::MarketDataKind::Trades as i32,
        })),
    }
}

/// 6 horas de trades de dos fuentes más latidos cada segundo, en orden K.
fn log() -> Vec<proto::Envelope> {
    let table = SourceRankTable::current();
    let mut rng = 99u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let end = start() + 6 * 3_600 * S;
    let mut streams = Vec::new();
    for src in [
        market(proto::Venue::Binance, "BTCUSDT"),
        market(proto::Venue::Binance, "ETHUSDT"),
    ] {
        let mut ts = start();
        let mut seq = 0;
        let mut price = 6_500_000i64;
        let mut v = Vec::new();
        while ts < end {
            ts += (next() % 20_000) as i64 * 1_000_000 + 1;
            seq += 1;
            price += (next() % 21) as i64 - 10;
            v.push(proto::Envelope {
                ts_event: ts - 2_000_000,
                ts_received: Some(ts),
                ts_available: ts,
                source: Some(src.clone()),
                source_seq: Some(seq),
                ingest_seq: seq,
                provenance: proto::Provenance::Live as i32,
                payload: Some(envelope::Payload::Market(proto::MarketEvent {
                    event: Some(market_event::Event::Trade(proto::Trade {
                        price: Some(Decimal::new(price.into(), 2).unwrap().into()),
                        qty_base: Some(
                            Decimal::new(i128::from(next() % 999 + 1), 3)
                                .unwrap()
                                .into(),
                        ),
                        aggressor_side: proto::Side::Buy as i32,
                        venue_trade_id: Some(seq.to_string()),
                    })),
                })),
            });
        }
        streams.push((table.source_order(&src).unwrap(), v.into_iter()));
    }
    let clock = proto::SourceId {
        kind: Some(proto::source_id::Kind::Clock(proto::ClockSource {})),
    };
    streams.push((
        table.source_order(&clock).unwrap(),
        synthetic_heartbeats(start(), end, S)
            .collect::<Vec<_>>()
            .into_iter(),
    ));
    Merge::new(streams).unwrap().collect()
}

fn config() -> KernelConfig {
    KernelConfig {
        timeframes: vec![Timeframe::Seconds(60), Timeframe::Seconds(3_600)],
        ..KernelConfig::default()
    }
}

/// Consumidor de ejemplo: por cada vela de 1m que cierra, emite (fuente, inicio, cierre)
/// y, si el cierre subió respecto de la anterior de esa fuente, lo marca.
#[derive(Default)]
struct CloseWatcher {
    last: std::collections::BTreeMap<String, Decimal>,
}

impl Consumer for CloseWatcher {
    type Output = (String, i64, (Decimal, bool));

    fn on_event(
        &mut self,
        _event: &proto::Envelope,
        past: &PastView<'_>,
        out: &mut Vec<Self::Output>,
    ) {
        for o in past.kernel_outputs() {
            if let KernelOutput::Bar(b) = o
                && b.bar.timeframe == Timeframe::Seconds(60)
            {
                let source = b.source.to_string();
                let up = self.last.get(&source).is_some_and(|p| b.bar.close > *p);
                self.last.insert(source.clone(), b.bar.close);
                out.push((source, b.bar.start, (b.bar.close, up)));
            }
        }
    }
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bender-runner-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn record(events: &[proto::Envelope], dir: &std::path::Path) -> Vec<String> {
    let mut store = Store::open(dir).unwrap();
    let mut sources = std::collections::BTreeSet::new();
    for e in events {
        let key = source_key(e.source.as_ref().unwrap());
        store
            .append_envelope(&key, "norm-1", &WriterOptions::default(), e)
            .unwrap();
        sources.insert(key);
    }
    store.close_all().unwrap();
    sources.into_iter().collect()
}

#[test]
fn same_outputs_in_replay_backtest_and_live() {
    let events = log();
    let dir = temp_dir("modes");
    let sources = record(&events, &dir);
    let store = Store::open(&dir).unwrap();

    // backtest sobre el log grabado.
    let mut src = StoreSource::open(&store, &sources, i64::MIN, i64::MAX).unwrap();
    let bt = backtest(config(), CloseWatcher::default(), &mut src).unwrap();
    assert!(bt.outputs.len() > 500, "{}", bt.outputs.len());
    assert!(
        !bt.manifest.segments.is_empty(),
        "el manifiesto lista los segmentos leídos"
    );

    // replay del mismo log, reproducido con un pacer que nunca frena.
    struct Free;
    impl Pacer for Free {
        fn wait_until(&mut self, _ts: i64) -> Pace {
            Pace::Continue
        }
    }
    let mut src = StoreSource::open(&store, &sources, i64::MIN, i64::MAX).unwrap();
    let mut replayed = Vec::new();
    while let Some(e) = bender_runner::EventSource::next_event(&mut src) {
        replayed.push(e.unwrap());
    }
    assert_eq!(replayed, events, "el store devuelve exactamente el log");
    let mut rp = Replay::new(config(), replayed, CloseWatcher::default);
    rp.play(&mut Free).unwrap();
    assert!(rp.is_finished());

    // live: un LiveSource falso que recibe el log por un canal (desde otro hilo) y lo
    // graba al mismo tiempo en otro store.
    let (tx, rx) = mpsc::channel();
    let feeder = {
        let events = events.clone();
        std::thread::spawn(move || {
            for e in events {
                tx.send(e).unwrap();
            }
        })
    };
    let live_dir = temp_dir("live");
    let mut live_src = LiveSource::new(rx).recording(Store::open(&live_dir).unwrap(), "norm-1");
    let lv = live(config(), CloseWatcher::default(), &mut live_src).unwrap();
    feeder.join().unwrap();
    let recorded = live_src.finish().unwrap().unwrap();

    assert_eq!(
        bt.manifest.output_hash,
        rp.manifest().output_hash,
        "backtest = replay"
    );
    assert_eq!(
        bt.manifest.output_hash, lv.manifest.output_hash,
        "backtest = live"
    );
    assert_eq!(bt.outputs, lv.outputs);
    assert_eq!(bt.manifest.config_hash, lv.manifest.config_hash);

    // Lo grabado en vivo se lee igual que el original.
    let mut again = StoreSource::open(&recorded, &sources, i64::MIN, i64::MAX).unwrap();
    let bt2 = backtest(config(), CloseWatcher::default(), &mut again).unwrap();
    assert_eq!(bt2.manifest.output_hash, bt.manifest.output_hash);
    std::fs::remove_dir_all(&dir).unwrap();
    std::fs::remove_dir_all(&live_dir).unwrap();
}

#[test]
fn replay_is_controllable_step_by_step() {
    let events = log();
    let mut rp = Replay::new(config(), events.clone(), CloseWatcher::default);
    assert_eq!(rp.step(1_000).unwrap(), 1_000);
    assert_eq!(rp.position(), 1_000);

    // Pausa: el pacer frena después de 500 eventos.
    struct PauseAfter(usize);
    impl Pacer for PauseAfter {
        fn wait_until(&mut self, _ts: i64) -> Pace {
            if self.0 == 0 {
                return Pace::Pause;
            }
            self.0 -= 1;
            Pace::Continue
        }
    }
    assert_eq!(rp.play(&mut PauseAfter(500)).unwrap(), 500);
    assert_eq!(rp.position(), 1_500);

    // Ir a T hacia adelante y hacia atrás: el estado es el de una corrida hasta T.
    for t in [start() + 4 * 3_600 * S, start() + 3_600 * S + 7] {
        rp.seek(t).unwrap();
        assert_eq!(rp.now().map(|n| n <= t), Some(true));
        let upto: Vec<_> = events
            .iter()
            .filter(|e| e.ts_available <= t)
            .cloned()
            .collect();
        let fresh = backtest(
            config(),
            CloseWatcher::default(),
            &mut bender_runner::VecSource::new(upto),
        )
        .unwrap();
        assert_eq!(
            rp.manifest().output_hash,
            fresh.manifest.output_hash,
            "seek a {t}"
        );
        assert_eq!(rp.outputs(), &fresh.outputs[..]);
    }
}

/// Features: lo que el consumidor ve del pasado. Una fila por trade.
#[derive(Default)]
struct Features {
    trades: u64,
}

impl Consumer for Features {
    type Output = DatasetRow;

    fn on_event(
        &mut self,
        event: &proto::Envelope,
        past: &PastView<'_>,
        out: &mut Vec<DatasetRow>,
    ) {
        let Some(envelope::Payload::Market(_)) = &event.payload else {
            return;
        };
        self.trades += 1;
        let key = source_key(event.source.as_ref().unwrap());
        let forming_close = past
            .bars(&key)
            .and_then(|b| b.forming(Timeframe::Seconds(60)).last())
            .map_or(0.0, |b| b.close.to_string().parse::<f64>().unwrap());
        out.push(DatasetRow {
            ts_available: event.ts_available,
            ingest_seq: event.ingest_seq,
            features: vec![
                ("trades".into(), self.trades as f64),
                ("close_1m".into(), forming_close),
            ],
            label: None,
        });
    }
}

/// Etiqueta: cuántos eventos hay en los próximos 60 s (mira el futuro, a propósito).
struct EventsAhead;

impl Labeler for EventsAhead {
    type Row = DatasetRow;
    fn horizon_ns(&self) -> i64 {
        60 * S
    }
    fn label(&mut self, _row: &DatasetRow, at: i64, future: &FutureWindow<'_>) -> f64 {
        assert!(
            future
                .events()
                .iter()
                .all(|e| e.ts_available > at && e.ts_available <= at + 60 * S)
        );
        future.events().len() as f64
    }
}

#[test]
fn dataset_features_do_not_change_if_the_future_is_truncated() {
    let events = log();
    let full = dataset(config(), Features::default(), &mut EventsAhead, &events).unwrap();
    assert!(full.iter().all(|r| r.label.is_some()));
    assert!(full.iter().any(|r| r.label.unwrap() > 0.0));
    for cut in [events.len() / 4, events.len() / 2, events.len() - 1] {
        let truncated = dataset(
            config(),
            Features::default(),
            &mut EventsAhead,
            &events[..cut],
        )
        .unwrap();
        let last_ts = events[cut - 1].ts_available;
        for (a, b) in full
            .iter()
            .zip(&truncated)
            .filter(|(a, _)| a.ts_available <= last_ts)
        {
            assert_eq!(
                (a.ts_available, a.ingest_seq, &a.features),
                (b.ts_available, b.ingest_seq, &b.features)
            );
        }
        assert_eq!(
            truncated.len(),
            full.iter().filter(|r| r.ts_available <= last_ts).count()
        );
    }
    let dir = temp_dir("dataset");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("features.parquet");
    write_dataset_parquet(&full, &path).unwrap();
    assert!(std::fs::metadata(&path).unwrap().len() > 1_000);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
fn future_window_cannot_be_built_by_a_consumer() {
    let t = trybuild::TestCases::new();
    t.compile_fail("tests/ui/*.rs");
}
