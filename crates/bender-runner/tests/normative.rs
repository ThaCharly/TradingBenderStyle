//! Criterios de #30: las cinco pruebas normativas con el consumidor de velas (incluidas
//! compuestas), la mutación que hace fallar al canario y el test dorado.

use std::process::Command;
use std::sync::Arc;

use bender_kernel::KernelState;
use bender_runner::golden::{DayHash, replay_day};
use bender_runner::normative::{self, BarsConsumer, synthetic_log};
use bender_runner::{Consumer, PastView, Runner, register_consumer_for_normative_tests};
use bender_store::{Store, WriterOptions};
use bender_types::{NS_PER_DAY, proto, source_key};

register_consumer_for_normative_tests!(bars, || BarsConsumer);

#[test]
fn bars_consumer_covers_bars_composites_and_revisions() {
    let log = synthetic_log(2);
    let run = bender_runner::backtest(
        log.config.clone(),
        BarsConsumer,
        &mut bender_runner::VecSource::new(log.events.clone()),
    )
    .unwrap();
    use bender_kernel::{BarEventKind, KernelOutput};
    let composites = run
        .outputs
        .iter()
        .filter(|o| matches!(o, KernelOutput::Composite(_)))
        .count();
    let revised = run
        .outputs
        .iter()
        .filter(|o| matches!(o, KernelOutput::Bar(b) if b.kind == BarEventKind::Revised))
        .count();
    assert!(composites > 100, "{composites}");
    assert!(revised > 10, "el log tiene tardíos: {revised}");
}

/// Doble corrida entre máquinas: el hash de las salidas sobre el log normativo está fijado
/// acá, así la CI (otra máquina, otra arquitectura si se agrega) lo verifica.
#[test]
fn double_run_hash_is_pinned() {
    let hash = normative::double_run(&synthetic_log(2), || BarsConsumer).unwrap();
    assert_eq!(
        hash, GOLDEN,
        "cambió la salida de las velas sobre el log normativo"
    );
}

const GOLDEN: &str = "83f3a07df714bef86f047f927e2bfcddcc1cfc6b4b217d8caf8783ba589d2079";

/// Mutación: un consumidor que lee el **próximo** evento del log (el futuro) por fuera de
/// la `PastView`, espiando el stream sobre el que corre.
struct PeeksTheFuture {
    log: Arc<Vec<proto::Envelope>>,
    seen: usize,
}

impl Consumer for PeeksTheFuture {
    type Output = i64;

    fn on_event(&mut self, _event: &proto::Envelope, _past: &PastView<'_>, out: &mut Vec<i64>) {
        self.seen += 1;
        out.push(self.log.get(self.seen).map_or(-1, |e| e.ts_available));
    }
}

#[test]
fn canary_catches_a_consumer_that_reads_the_future() {
    let log = synthetic_log(1);
    let at: Vec<i64> = [3, 2]
        .iter()
        .map(|d| log.events[log.events.len() / d].ts_available + 1)
        .collect();
    // Honesto: pasa.
    normative::future_canary(&log, || BarsConsumer, &at).unwrap();
    // Tramposo: falla.
    let result = normative::future_canary_with(
        &log,
        |events| PeeksTheFuture {
            log: Arc::new(events.to_vec()),
            seen: 0,
        },
        &at,
    );
    let err = result.expect_err("el canario tiene que fallar con un consumidor que mira el futuro");
    assert!(err.contains("cambió salidas anteriores"), "{err}");
}

/// Test dorado: la corrida "en vivo" anota el hash de cada día; `bender-golden` replaya el
/// día desde lo grabado (con el checkpoint del inicio del día) y coincide. Con un hash
/// equivocado, falla.
#[test]
fn golden_tool_matches_live_day_hashes() {
    let log = synthetic_log(2);
    let dir = std::env::temp_dir().join(format!("bender-golden-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let mut store = Store::open(&dir).unwrap();

    // "Vivo": procesa, graba, toma el checkpoint de cada día y anota el hash del día.
    let mut runner = Runner::new(log.config.clone(), BarsConsumer);
    let mut out = Vec::new();
    let mut days: Vec<(i64, DayHash)> = Vec::new();
    for e in &log.events {
        let day = e.ts_available.div_euclid(NS_PER_DAY) * NS_PER_DAY;
        if days.last().is_none_or(|(d, _)| *d != day) {
            store
                .put_checkpoint("kernel", day, &runner.state().checkpoint())
                .unwrap();
            days.push((day, DayHash::default()));
        }
        let key = source_key(e.source.as_ref().unwrap());
        store
            .append_envelope(&key, "normativo-1", &WriterOptions::default(), e)
            .unwrap();
        out.clear();
        runner.process(e, &mut out).unwrap();
        let today = &mut days.last_mut().unwrap().1;
        today.events += 1;
        for o in &out {
            today.push(o);
        }
    }
    store.close_all().unwrap();

    // replay_day sobre el mismo día da el mismo hash.
    let (day1, live1) = &days[1];
    let cp = KernelState::from_checkpoint(&store.get_checkpoint("kernel", *day1).unwrap().unwrap())
        .unwrap();
    let replayed = replay_day(cp, BarsConsumer, *day1, log.events.iter().cloned()).unwrap();
    assert_eq!(&replayed, live1);

    // Y el binario, sobre el store grabado.
    let bin = env!("CARGO_BIN_EXE_bender-golden");
    let date = "2026-10-02"; // el segundo día del log normativo
    let ok = Command::new(bin)
        .args([
            "--store",
            dir.to_str().unwrap(),
            "--day",
            date,
            "--expected",
            &live1.hex(),
        ])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stdout)
    );
    let bad = Command::new(bin)
        .args([
            "--store",
            dir.to_str().unwrap(),
            "--day",
            date,
            "--expected",
            "00",
        ])
        .output()
        .unwrap();
    assert_eq!(bad.status.code(), Some(1));
    std::fs::remove_dir_all(&dir).unwrap();
}
