//! `bender-recorder`: grabador de datos públicos de Binance. Ver `docs/venues/binance.md`.
//!
//! - `bender-recorder run --store <carpeta> [--config <archivo.toml>]`: graba hasta Ctrl+C.
//! - `bender-recorder report --store <carpeta>`: reporte en Markdown.
//! - `bender-recorder capture --out <carpeta> [--symbol BTCUSDT] [--seconds 120]`:
//!   fixtures reales para los tests del normalizador (`--usdm-ws` cambia la URL del WS
//!   de perps).

use std::process::ExitCode;

use bender_recorder::config::Config;
use bender_recorder::report::Report;
use bender_store::Store;

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn load_config(args: &[String]) -> Result<Config, String> {
    match arg(args, "--config") {
        Some(path) => {
            let text = std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?;
            Config::from_toml(&text).map_err(|e| format!("{path}: {e}"))
        }
        None => Ok(Config::default()),
    }
}

const USAGE: &str = "uso:
  bender-recorder run --store <carpeta> [--config <archivo.toml>]
  bender-recorder report --store <carpeta>
  bender-recorder capture --out <carpeta> [--symbol BTCUSDT] [--seconds 120] [--usdm-ws <url>] [--config <archivo.toml>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().collect();
    let result = match args.get(1).map(String::as_str) {
        Some("run") => run(&args),
        Some("report") => report(&args),
        Some("capture") => capture(&args),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn runtime() -> Result<tokio::runtime::Runtime, String> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::runtime::Runtime::new().map_err(|e| e.to_string())
}

fn run(args: &[String]) -> Result<(), String> {
    let store_dir = arg(args, "--store").ok_or(USAGE)?;
    let config = load_config(args)?;
    let store = Store::open(&store_dir).map_err(|e| e.to_string())?;
    eprintln!(
        "[bender-recorder] grabando {} instrumentos en {store_dir} (Ctrl+C para terminar)",
        config.instruments.len()
    );
    let rt = runtime()?;
    let store = rt.block_on(bender_recorder::io::run(config, store, async {
        let _ = tokio::signal::ctrl_c().await;
    }))?;
    eprintln!(
        "[bender-recorder] {} segmentos en el store",
        store.segments().len()
    );
    Ok(())
}

fn report(args: &[String]) -> Result<(), String> {
    let store_dir = arg(args, "--store").ok_or(USAGE)?;
    let store = Store::open(&store_dir).map_err(|e| e.to_string())?;
    let report = Report::from_store(&store).map_err(|e| e.to_string())?;
    print!("{}", report.markdown());
    Ok(())
}

fn capture(args: &[String]) -> Result<(), String> {
    let out = arg(args, "--out").ok_or(USAGE)?;
    let symbol = arg(args, "--symbol")
        .unwrap_or_else(|| "BTCUSDT".into())
        .to_ascii_uppercase();
    let seconds = arg(args, "--seconds")
        .and_then(|s| s.parse().ok())
        .unwrap_or(120);
    let mut config = load_config(args)?;
    if let Some(url) = arg(args, "--usdm-ws") {
        config.endpoints.usdm_ws = url;
    }
    let rt = runtime()?;
    let written = rt.block_on(bender_recorder::capture::capture(
        &config,
        &symbol,
        seconds,
        std::path::Path::new(&out),
    ))?;
    for w in written {
        println!("{out}/{w}.json");
    }
    Ok(())
}
