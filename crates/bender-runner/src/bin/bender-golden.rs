//! `bender-golden`: test dorado "vivo = replay" (doc 05 §11.3). Ver
//! `docs/operacion/test-dorado.md`.
//!
//! Uso: `bender-golden --store <carpeta> --day AAAA-MM-DD --expected <hash>`
//!
//! Replaya el día desde la zona normalizada del store con el consumidor de velas,
//! partiendo del checkpoint `kernel` del inicio del día (o desde el principio de lo
//! grabado si no hay checkpoint), y compara el hash de las salidas con el que anotó la
//! corrida en vivo. Sale con 0 si coinciden y con 1 si no.

use std::process::ExitCode;

use bender_kernel::{KernelConfig, KernelState};
use bender_runner::golden::{day_line, replay_day};
use bender_runner::normative::BarsConsumer;
use bender_runner::{EventSource, Runner, StoreSource};
use bender_store::{Store, Zone};
use bender_types::{NS_PER_DAY, days_from_civil};

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn parse_day(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y = parts.next()?.parse().ok()?;
    let m = parts.next()?.parse().ok()?;
    let d = parts.next()?.parse().ok()?;
    Some(days_from_civil(y, m, d) * NS_PER_DAY)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let (Some(store_dir), Some(day), Some(expected)) = (
        arg(&args, "--store"),
        arg(&args, "--day"),
        arg(&args, "--expected"),
    ) else {
        eprintln!("uso: bender-golden --store <carpeta> --day AAAA-MM-DD --expected <hash>");
        return ExitCode::from(2);
    };
    let Some(day_start) = parse_day(&day) else {
        eprintln!("día inválido: {day}");
        return ExitCode::from(2);
    };
    let store = match Store::open(&store_dir) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("no se pudo abrir el store: {e}");
            return ExitCode::from(2);
        }
    };
    let mut sources: Vec<String> = store
        .segments()
        .iter()
        .filter(|s| s.zone == Zone::Normalized)
        .map(|s| s.source.clone())
        .collect();
    sources.sort();
    sources.dedup();

    let checkpoint = store.get_checkpoint("kernel", day_start).ok().flatten();
    let (state, from) = match checkpoint.map(|b| KernelState::from_checkpoint(&b)) {
        Some(Ok(state)) => (state, day_start),
        Some(Err(e)) => {
            eprintln!("checkpoint del {day} inválido ({e}): se replaya desde el principio");
            (KernelState::new(KernelConfig::default()), i64::MIN)
        }
        None => (KernelState::new(KernelConfig::default()), i64::MIN),
    };
    // Sin checkpoint, se lleva el núcleo hasta el inicio del día con lo grabado antes.
    let state = if from == i64::MIN {
        let mut runner = Runner::from_state(state, BarsConsumer);
        let mut before = match StoreSource::open(&store, &sources, i64::MIN, day_start) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        };
        let mut out = Vec::new();
        while let Some(e) = before.next_event() {
            let r = e.map_err(|e| e.to_string()).and_then(|e| {
                out.clear();
                runner.process(&e, &mut out).map_err(|e| e.to_string())
            });
            if let Err(e) = r {
                eprintln!("{e}");
                return ExitCode::from(2);
            }
        }
        runner.state().clone()
    } else {
        state
    };
    let mut day_events = Vec::new();
    match StoreSource::open(&store, &sources, day_start, day_start + NS_PER_DAY) {
        Ok(mut src) => {
            while let Some(e) = src.next_event() {
                match e {
                    Ok(e) => day_events.push(e),
                    Err(e) => {
                        eprintln!("{e}");
                        return ExitCode::from(2);
                    }
                }
            }
        }
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    }
    let replayed = match replay_day(state, BarsConsumer, day_start, day_events) {
        Ok(h) => h,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(2);
        }
    };
    println!("replay: {}", day_line(&day, &replayed));
    if replayed.hex() == expected.trim() {
        println!("OK: vivo = replay");
        ExitCode::SUCCESS
    } else {
        println!("DIFERENCIA: en vivo se anotó {expected}");
        ExitCode::FAILURE
    }
}
