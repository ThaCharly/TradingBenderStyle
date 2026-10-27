//! Las cinco pruebas normativas del núcleo temporal (doc 05 §11, doc 02 §9), como suite
//! reutilizable: cualquier [`Consumer`] se registra con
//! [`register_consumer_for_normative_tests!`](crate::register_consumer_for_normative_tests)
//! y obtiene un `#[test]` por prueba.
//!
//! 1. **Incremental = batch**: procesar evento a evento (replay paso a paso) da el mismo
//!    hash que la corrida en lote.
//! 2. **Doble corrida**: dos corridas dan el mismo hash. Entre máquinas lo cubren el hash
//!    fijado en los tests de la CI y `bender-golden` (servidor y PC).
//! 3. **Vivo = replay**: la corrida en vivo (con [`LiveSource`]) y el replay de lo que
//!    grabó dan las mismas salidas.
//! 4. **Canario de futuro**: un evento con `ts_available` futuro no cambia ninguna salida
//!    emitida antes de ese instante.
//! 5. **Checkpoint = desde cero**: retomar desde el checkpoint de un día (núcleo y
//!    consumidor) da las mismas salidas y el mismo estado final que correr desde cero.
//!
//! Cada prueba devuelve `Err` con una explicación si falla.

use std::sync::mpsc;

use bender_kernel::canonical::{self, Checkpointable};
use bender_kernel::{
    CompositeDefinition, KernelConfig, KernelOutput, KernelState, Merge, PriceRule,
    SourceRankTable, Timeframe, synthetic_heartbeats, user_envelope,
};
use bender_types::proto::{self, envelope, market_event, user_event};
use bender_types::{Decimal, NS_PER_DAY, Ulid, days_from_civil};

use crate::{Consumer, LiveSource, PastView, Replay, RunError, Runner, VecSource, backtest};

/// Log y configuración sobre los que corren las pruebas.
#[derive(Clone, Debug)]
pub struct NormativeLog {
    pub config: KernelConfig,
    pub events: Vec<proto::Envelope>,
}

/// Consumidor de velas: emite todo lo que el núcleo cierra o revisa (velas por fuente y
/// compuestas). Es el consumidor con el que corren las pruebas normativas en la CI.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BarsConsumer;

impl Consumer for BarsConsumer {
    type Output = KernelOutput;

    fn on_event(
        &mut self,
        _event: &proto::Envelope,
        past: &PastView<'_>,
        out: &mut Vec<KernelOutput>,
    ) {
        out.extend(past.kernel_outputs().iter().cloned());
    }
}

impl Checkpointable for BarsConsumer {
    fn encode(&self, _: &mut canonical::Encoder) {}
    fn decode(_: &mut canonical::Decoder<'_>) -> Result<Self, canonical::CheckpointError> {
        Ok(Self)
    }
}

const S: i64 = 1_000_000_000;

fn market(venue: proto::Venue, market_type: proto::MarketType, symbol: &str) -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: venue as i32,
                market_type: market_type as i32,
                symbol: symbol.into(),
            }),
            data_kind: proto::MarketDataKind::Trades as i32,
        })),
    }
}

/// Log sintético de `days` días que ejercita todo el núcleo: trades de tres fuentes (dos
/// en una compuesta), trades tardíos (revisiones), un hueco con recuperación parcial,
/// objetos del usuario y latidos cada segundo. Determinista.
pub fn synthetic_log(days: i64) -> NormativeLog {
    let table = SourceRankTable::current();
    let start = days_from_civil(2026, 10, 1) * NS_PER_DAY;
    let end = start + days * NS_PER_DAY;
    let sources = [
        market(
            proto::Venue::Binance,
            proto::MarketType::PerpLinear,
            "BTCUSDT",
        ),
        market(
            proto::Venue::Bybit,
            proto::MarketType::PerpLinear,
            "BTCUSDT",
        ),
        market(proto::Venue::Binance, proto::MarketType::Spot, "BTCUSDT"),
    ];
    let keys: Vec<String> = sources.iter().map(bender_types::source_key).collect();
    let mut rng = 0x5eed_0000_0000_0001u64;
    let mut next = move || {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        rng
    };
    let mut streams: Vec<Vec<proto::Envelope>> = Vec::new();
    for src in &sources {
        let mut ts = start;
        let mut seq = 0u64;
        let mut price = 6_500_000i64;
        let mut v = Vec::new();
        while ts < end {
            ts += (next() % 30_000) as i64 * 1_000_000 + 1;
            seq += 1;
            price += (next() % 41) as i64 - 20;
            // Uno de cada 50 trades llega tarde: ts_event de hasta 90 s antes.
            let late = if next() % 50 == 0 {
                (next() % 90) as i64 * S
            } else {
                0
            };
            v.push(proto::Envelope {
                ts_event: ts - 2_000_000 - late,
                ts_received: Some(ts),
                ts_available: ts,
                source: Some(src.clone()),
                source_seq: Some(seq),
                ingest_seq: seq,
                provenance: proto::Provenance::Live as i32,
                payload: Some(envelope::Payload::Market(proto::MarketEvent {
                    event: Some(market_event::Event::Trade(proto::Trade {
                        price: Some(Decimal::new(price.into(), 2).expect("precio").into()),
                        qty_base: Some(
                            Decimal::new(i128::from(next() % 2_000 + 1), 3)
                                .expect("qty")
                                .into(),
                        ),
                        aggressor_side: if next() % 2 == 0 {
                            proto::Side::Buy
                        } else {
                            proto::Side::Sell
                        } as i32,
                        venue_trade_id: Some(seq.to_string()),
                    })),
                })),
            });
        }
        streams.push(v);
    }
    // Hueco de Bybit de 10 min el primer día, recuperado a medias una hora después.
    let gap_id: proto::Ulid = Ulid::from_parts(1, 7).into();
    let gap_from = start + 6 * 3_600 * S;
    let gap_to = gap_from + 600 * S;
    let mut gaps = vec![
        proto::Envelope {
            ts_event: gap_to,
            ts_available: gap_to,
            source: Some(sources[1].clone()),
            ingest_seq: u64::MAX - 1,
            provenance: proto::Provenance::Live as i32,
            payload: Some(envelope::Payload::Gap(proto::Gap {
                gap_id: Some(gap_id.clone()),
                source: Some(sources[1].clone()),
                from_ts: gap_from,
                to_ts: gap_to,
                reason: proto::GapReason::Disconnected as i32,
                detail: None,
            })),
            ..Default::default()
        },
        proto::Envelope {
            ts_event: gap_to + 3_600 * S,
            ts_available: gap_to + 3_600 * S,
            source: Some(sources[1].clone()),
            ingest_seq: u64::MAX,
            provenance: proto::Provenance::Recovered as i32,
            payload: Some(envelope::Payload::GapRecovery(proto::GapRecovery {
                gap_id: Some(gap_id),
                recovered_from: Some(gap_from),
                recovered_to: Some(gap_from + 300 * S),
                method: proto::RecoveryMethod::RestById as i32,
                result: proto::RecoveryResult::Partial as i32,
            })),
            ..Default::default()
        },
    ];
    // Los eventos de huecos van en el stream de Bybit, en orden K.
    let bybit = &mut streams[1];
    bybit.append(&mut gaps);
    bybit.sort_by_key(|e| (e.ts_available, e.source_seq, e.ingest_seq));
    // ingest_seq propio y creciente dentro de la fuente, en el orden K resultante.
    for (i, e) in bybit.iter_mut().enumerate() {
        e.ingest_seq = i as u64 + 1;
    }
    let user: Vec<proto::Envelope> = (0..(days * 8) as u64)
        .map(|i| {
            let ev = proto::UserEvent {
                author: None,
                event: Some(user_event::Event::DrawingUpserted(proto::DrawingUpserted {
                    drawing_id: format!("d{}", i % 5),
                    payload: Some(proto::OpaquePayload {
                        version: 1,
                        data: i.to_be_bytes().to_vec(),
                    }),
                })),
            };
            user_envelope(
                "drawings",
                i + 1,
                None,
                start + i as i64 * 3 * 3_600 * S + 11,
                ev,
            )
        })
        .collect();
    streams.push(user);
    streams.push(synthetic_heartbeats(start, end, S).collect());
    let ordered = streams.into_iter().filter(|v| !v.is_empty()).map(|v| {
        let order = table
            .source_order(v[0].source.as_ref().expect("fuente"))
            .expect("source_rank");
        (order, v.into_iter())
    });
    let events = Merge::new(ordered).expect("fuentes distintas").collect();
    let config = KernelConfig {
        timeframes: vec![
            Timeframe::Seconds(60),
            Timeframe::Seconds(900),
            Timeframe::Seconds(14_400),
            Timeframe::Seconds(86_400),
        ],
        composites: vec![CompositeDefinition {
            id: "BTC/USD perps agregado".into(),
            version: 1,
            members: vec![keys[0].clone(), keys[1].clone()],
            price_rule: PriceRule::Vwap,
        }],
        ..KernelConfig::default()
    };
    NormativeLog { config, events }
}

/// Salidas de una corrida etiquetadas con el `ts_available` del evento que las produjo.
fn tagged_outputs<C: Consumer>(
    log: &NormativeLog,
    events: &[proto::Envelope],
    consumer: C,
) -> Result<Vec<(i64, Vec<u8>)>, RunError> {
    let mut runner = Runner::new(log.config.clone(), consumer);
    let mut out = Vec::new();
    let mut tagged = Vec::new();
    for e in events {
        out.clear();
        runner.process(e, &mut out)?;
        tagged.extend(out.iter().map(|o| (e.ts_available, canonical::to_bytes(o))));
    }
    Ok(tagged)
}

fn err(e: RunError) -> String {
    e.to_string()
}

/// Prueba 1: Incremental = batch.
pub fn incremental_equals_batch<C: Consumer>(
    log: &NormativeLog,
    make: impl Fn() -> C,
) -> Result<(), String> {
    let batch = backtest(
        log.config.clone(),
        make(),
        &mut VecSource::new(log.events.clone()),
    )
    .map_err(err)?;
    let mut replay = Replay::new(log.config.clone(), log.events.clone(), &make);
    while replay.step(1).map_err(err)? == 1 {}
    let incremental = replay.manifest();
    if incremental.output_hash != batch.manifest.output_hash {
        return Err(format!(
            "incremental {} ≠ batch {}",
            incremental.output_hash, batch.manifest.output_hash
        ));
    }
    if replay.runner().state().state_hash() != batch.runner.state().state_hash() {
        return Err("el estado final del núcleo difiere".into());
    }
    Ok(())
}

/// Prueba 2: Doble corrida: devuelve el hash, igual en las dos.
pub fn double_run<C: Consumer>(log: &NormativeLog, make: impl Fn() -> C) -> Result<String, String> {
    let a = backtest(
        log.config.clone(),
        make(),
        &mut VecSource::new(log.events.clone()),
    )
    .map_err(err)?;
    let b = backtest(
        log.config.clone(),
        make(),
        &mut VecSource::new(log.events.clone()),
    )
    .map_err(err)?;
    if a.manifest != b.manifest {
        return Err(format!("{:?} ≠ {:?}", a.manifest, b.manifest));
    }
    Ok(a.manifest.output_hash)
}

/// Prueba 3: Vivo = replay: el log llega por un canal a una [`LiveSource`] que lo graba; el
/// replay de lo grabado da las mismas salidas.
pub fn live_equals_replay<C: Consumer>(
    log: &NormativeLog,
    make: impl Fn() -> C,
) -> Result<(), String> {
    let dir = std::env::temp_dir().join(format!(
        "bender-normative-live-{}-{}",
        std::process::id(),
        log.events.len()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let store = bender_store::Store::open(&dir).map_err(|e| e.to_string())?;
    let (tx, rx) = mpsc::channel();
    let events = log.events.clone();
    let feeder = std::thread::spawn(move || {
        for e in events {
            if tx.send(e).is_err() {
                break;
            }
        }
    });
    let mut live_src = LiveSource::new(rx).recording(store, "normativo-1");
    let live = crate::live(log.config.clone(), make(), &mut live_src).map_err(err)?;
    feeder
        .join()
        .map_err(|_| "el hilo de alimentación falló".to_string())?;
    let store = live_src
        .finish()
        .map_err(|e| e.to_string())?
        .ok_or("sin store")?;
    let mut sources: Vec<String> = store.segments().iter().map(|s| s.source.clone()).collect();
    sources.sort();
    sources.dedup();
    let mut recorded = crate::StoreSource::open(&store, &sources, i64::MIN, i64::MAX)
        .map_err(|e| e.to_string())?;
    let mut replayed = Vec::new();
    while let Some(e) = crate::EventSource::next_event(&mut recorded) {
        replayed.push(e.map_err(|e| e.to_string())?);
    }
    let _ = std::fs::remove_dir_all(&dir);
    let mut replay = Replay::new(log.config.clone(), replayed, &make);
    while replay.step(10_000).map_err(err)? > 0 {}
    if replay.manifest().output_hash != live.manifest.output_hash {
        return Err(format!(
            "vivo {} ≠ replay {}",
            live.manifest.output_hash,
            replay.manifest().output_hash
        ));
    }
    Ok(())
}

/// Evento canario: un trade de un venue que no aparece en el log, con `ts_available =
/// at`. No puede afectar nada anterior a `at`.
pub fn canary_event(at: i64) -> proto::Envelope {
    proto::Envelope {
        ts_event: at,
        ts_received: Some(at),
        ts_available: at,
        source: Some(market(
            proto::Venue::Paper,
            proto::MarketType::Spot,
            "CANARIO",
        )),
        source_seq: Some(1),
        ingest_seq: 1,
        provenance: proto::Provenance::Synthetic as i32,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(market_event::Event::Trade(proto::Trade {
                price: Some(Decimal::new(123_456, 2).expect("precio").into()),
                qty_base: Some(Decimal::new(1, 0).expect("qty").into()),
                aggressor_side: proto::Side::Buy as i32,
                venue_trade_id: None,
            })),
        })),
    }
}

/// Inserta el canario en orden K.
pub fn with_canary(events: &[proto::Envelope], at: i64) -> Vec<proto::Envelope> {
    let table = SourceRankTable::current();
    let canary = canary_event(at);
    let order = table
        .source_order(canary.source.as_ref().expect("fuente"))
        .expect("PAPER tiene rango");
    let rest = events.to_vec();
    // El log es una fusión de streams; se vuelve a fusionar con el stream del canario.
    let mut by_source: std::collections::BTreeMap<String, Vec<proto::Envelope>> =
        Default::default();
    for e in rest {
        by_source
            .entry(bender_types::source_key(e.source.as_ref().expect("fuente")))
            .or_default()
            .push(e);
    }
    let streams = by_source.into_values().map(|v| {
        let o = table
            .source_order(v[0].source.as_ref().expect("fuente"))
            .expect("rango");
        (o, v.into_iter())
    });
    Merge::new(streams.chain(std::iter::once((order, vec![canary].into_iter()))))
        .expect("fuentes distintas")
        .collect()
}

/// Prueba 4: Canario de futuro: con un evento futuro inyectado en cada instante de `at`, todas
/// las salidas emitidas antes de ese instante son idénticas a las de la corrida sin él.
pub fn future_canary<C: Consumer>(
    log: &NormativeLog,
    make: impl Fn() -> C,
    at: &[i64],
) -> Result<(), String> {
    future_canary_with(log, |_| make(), at)
}

/// Como [`future_canary`], pero la fábrica recibe los eventos de la corrida que va a
/// hacer. Existe para los tests de mutación: un consumidor tramposo que espía el log por
/// fuera de la `PastView` tiene que hacer fallar al canario.
pub fn future_canary_with<C: Consumer>(
    log: &NormativeLog,
    make: impl Fn(&[proto::Envelope]) -> C,
    at: &[i64],
) -> Result<(), String> {
    let base = tagged_outputs(log, &log.events, make(&log.events)).map_err(err)?;
    for &t in at {
        let poisoned = with_canary(&log.events, t);
        let got = tagged_outputs(log, &poisoned, make(&poisoned)).map_err(err)?;
        let before = |v: &[(i64, Vec<u8>)]| -> Vec<(i64, Vec<u8>)> {
            v.iter().filter(|(ts, _)| *ts < t).cloned().collect()
        };
        let (a, b) = (before(&base), before(&got));
        if a != b {
            let first = a
                .iter()
                .zip(&b)
                .position(|(x, y)| x != y)
                .unwrap_or(a.len().min(b.len()));
            return Err(format!(
                "el canario en {t} cambió salidas anteriores (primera diferencia en la salida #{first})"
            ));
        }
    }
    Ok(())
}

/// Prueba 5: Checkpoint = desde cero: en el inicio de cada día del log se toma un checkpoint del
/// núcleo y del consumidor; retomar desde ahí da las mismas salidas posteriores y el mismo
/// estado final que la corrida completa.
pub fn checkpoint_equals_scratch<C>(log: &NormativeLog, make: impl Fn() -> C) -> Result<(), String>
where
    C: Consumer + Checkpointable,
{
    let full = tagged_outputs(log, &log.events, make()).map_err(err)?;
    let mut scratch = Runner::new(log.config.clone(), make());
    let mut out = Vec::new();
    let mut checkpoints = Vec::new();
    let mut day = None;
    for (i, e) in log.events.iter().enumerate() {
        let d = e.ts_available.div_euclid(NS_PER_DAY);
        if day.is_some_and(|prev| prev != d) {
            checkpoints.push((
                i,
                scratch.state.checkpoint(),
                canonical::to_bytes(&scratch.consumer),
            ));
        }
        day = Some(d);
        scratch.process(e, &mut out).map_err(err)?;
    }
    if checkpoints.is_empty() {
        return Err("el log tiene que cubrir más de un día".into());
    }
    for (i, kernel_bytes, consumer_bytes) in checkpoints {
        let state = KernelState::from_checkpoint(&kernel_bytes).map_err(|e| e.to_string())?;
        let consumer: C = canonical::from_bytes(&consumer_bytes).map_err(|e| e.to_string())?;
        let mut resumed = Runner::from_state(state, consumer);
        let mut tagged = Vec::new();
        for e in &log.events[i..] {
            out.clear();
            resumed.process(e, &mut out).map_err(err)?;
            tagged.extend(out.iter().map(|o| (e.ts_available, canonical::to_bytes(o))));
        }
        let from = log.events[i].ts_available;
        let expected: Vec<_> = full.iter().filter(|(ts, _)| *ts >= from).cloned().collect();
        if tagged != expected {
            return Err(format!(
                "desde el checkpoint del evento #{i} las salidas difieren"
            ));
        }
        if resumed.state.state_hash() != scratch.state.state_hash() {
            return Err(format!(
                "desde el checkpoint del evento #{i} el estado final difiere"
            ));
        }
        if canonical::to_bytes(&resumed.consumer) != canonical::to_bytes(&scratch.consumer) {
            return Err(format!(
                "desde el checkpoint del evento #{i} el consumidor difiere"
            ));
        }
    }
    Ok(())
}

/// Registra un consumidor en la suite normativa: genera un módulo `$name` con un
/// `#[test]` por prueba, sobre [`synthetic_log`] de 2 días. `$make` construye el
/// consumidor; tiene que implementar `Consumer + Checkpointable`.
#[macro_export]
macro_rules! register_consumer_for_normative_tests {
    ($name:ident, $make:expr) => {
        mod $name {
            #[allow(unused_imports)]
            use super::*;

            fn log() -> &'static $crate::normative::NormativeLog {
                static LOG: ::std::sync::OnceLock<$crate::normative::NormativeLog> =
                    ::std::sync::OnceLock::new();
                LOG.get_or_init(|| $crate::normative::synthetic_log(2))
            }

            #[test]
            fn incremental_equals_batch() {
                $crate::normative::incremental_equals_batch(log(), $make).unwrap();
            }

            #[test]
            fn double_run() {
                $crate::normative::double_run(log(), $make).unwrap();
            }

            #[test]
            fn live_equals_replay() {
                $crate::normative::live_equals_replay(log(), $make).unwrap();
            }

            #[test]
            fn future_canary() {
                let events = &log().events;
                let at = [
                    events[events.len() / 5].ts_available + 1,
                    events[events.len() / 2].ts_available + 1,
                    events[events.len() * 4 / 5].ts_available + 1,
                ];
                $crate::normative::future_canary(log(), $make, &at).unwrap();
            }

            #[test]
            fn checkpoint_equals_scratch() {
                $crate::normative::checkpoint_equals_scratch(log(), $make).unwrap();
            }
        }
    };
}
