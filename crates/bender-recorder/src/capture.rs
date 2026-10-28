//! `bender-recorder capture`: graba fixtures reales para los tests del normalizador
//! (`crates/bender-venue-binance/tests/fixtures/reales/`). Por mercado guarda el primer
//! mensaje de cada stream, un snapshot del libro, el primer delta que engancha con ese
//! snapshot, una página REST de `aggTrades` y el open interest; al lado de cada uno, el
//! `.esperado` que genera el normalizador actual (revisarlo antes de commitear).
//!
//! Los datos son públicos y de mercado: no hay nada personal.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use bender_venue_binance::book::{DiffOutcome, LocalBook, SnapshotOutcome};
use bender_venue_binance::{Market, Parsed, render};
use futures_util::StreamExt;
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;

fn prefix(market: Market) -> &'static str {
    match market {
        Market::Spot => "spot",
        Market::UsdM => "usdm",
    }
}

async fn get(client: &reqwest::Client, url: &str) -> Result<Vec<u8>, String> {
    let resp = client.get(url).send().await.map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("{url}: HTTP {}", resp.status()));
    }
    Ok(resp.bytes().await.map_err(|e| e.to_string())?.to_vec())
}

/// Sin espacios ni saltos de línea alrededor.
fn compact(raw: &[u8]) -> &[u8] {
    raw.trim_ascii()
}

/// Captura los fixtures de `symbol` en los mercados configurados y los escribe en `out`.
pub async fn capture(
    config: &Config,
    symbol: &str,
    seconds: u64,
    out: &Path,
) -> Result<Vec<String>, String> {
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())?;
    let mut written = Vec::new();
    let lower = symbol.to_ascii_lowercase();
    for market in [Market::Spot, Market::UsdM] {
        let mut streams = vec![
            format!("{lower}@aggTrade"),
            format!("{lower}@depth@100ms"),
            format!("{lower}@bookTicker"),
        ];
        if market == Market::UsdM {
            streams.push(format!("{lower}@markPrice@1s"));
            streams.push(format!("{lower}@forceOrder"));
        }
        let url = format!(
            "{}/stream?streams={}",
            config.endpoints.ws(market).trim_end_matches('/'),
            streams.join("/")
        );
        let (mut ws, _) = tokio_tungstenite::connect_async(url.as_str())
            .await
            .map_err(|e| format!("{url}: {e}"))?;
        let mut firsts: BTreeMap<&'static str, Vec<u8>> = BTreeMap::new();
        let mut diffs = Vec::new();
        let mut last_agg_id = None;
        let mut snapshot: Option<Vec<u8>> = None;
        let mut delta: Option<Vec<u8>> = None;
        // Mensajes recibidos por stream, deltas desde el último snapshot y snapshots
        // pedidos (para diagnosticar desde la terminal).
        let mut counts: BTreeMap<String, u64> = BTreeMap::new();
        let mut since_snapshot = 0u32;
        let mut snapshots = 0u32;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
        let rest = config.endpoints.rest(market).trim_end_matches('/');
        let api = match market {
            Market::Spot => "/api/v3",
            Market::UsdM => "/fapi/v1",
        };
        loop {
            let wanted = if market == Market::UsdM { 4 } else { 2 };
            if firsts.len() >= wanted && delta.is_some() {
                break;
            }
            let msg = tokio::select! {
                m = ws.next() => m,
                _ = tokio::time::sleep_until(deadline) => break,
            };
            let raw = match msg {
                Some(Ok(Message::Text(t))) => t.as_bytes().to_vec(),
                Some(Ok(_)) => continue,
                Some(Err(e)) => return Err(e.to_string()),
                None => break,
            };
            let Ok(parsed) = bender_venue_binance::parse(&raw) else {
                *counts.entry("(no reconocido)".into()).or_default() += 1;
                continue;
            };
            let stream = match &parsed {
                Parsed::AggTrade(_) => "aggTrade",
                Parsed::Depth(_) => "depth",
                Parsed::BookTicker(_) => "bookTicker",
                Parsed::MarkPrice(_) => "markPrice",
                Parsed::ForceOrder(_) => "forceOrder",
            };
            *counts.entry(stream.into()).or_default() += 1;
            let name = match &parsed {
                Parsed::AggTrade(t) => {
                    last_agg_id = Some(t.agg_id);
                    "aggtrade"
                }
                Parsed::BookTicker(_) => "bookticker",
                Parsed::MarkPrice(_) => "markprice",
                Parsed::ForceOrder(_) => "forceorder",
                Parsed::Depth(d) => {
                    diffs.push((d.clone(), raw.clone()));
                    since_snapshot += 1;
                    // Si el snapshot quedó viejo (ningún delta engancha), se pide otro.
                    let stale = snapshot.is_some() && delta.is_none() && since_snapshot >= 30;
                    if stale {
                        diffs.drain(..diffs.len().saturating_sub(5));
                    }
                    if (snapshot.is_none() && diffs.len() >= 5) || stale {
                        since_snapshot = 0;
                        snapshots += 1;
                        snapshot = Some(
                            get(
                                &client,
                                &format!("{rest}{api}/depth?symbol={symbol}&limit=50"),
                            )
                            .await?,
                        );
                    }
                    if let (Some(snap), None) = (&snapshot, &delta) {
                        let parsed_snap = bender_venue_binance::parse_depth_snapshot(snap)
                            .map_err(|e| e.to_string())?;
                        for (d, raw) in &diffs {
                            let mut book = LocalBook::new(market, render::FIXTURE_TOP_N);
                            if !matches!(
                                book.on_snapshot(&parsed_snap),
                                Ok(SnapshotOutcome::Synced { .. })
                            ) {
                                break;
                            }
                            if matches!(book.on_diff(d), Ok(DiffOutcome::Applied(Some(_)))) {
                                delta = Some(raw.clone());
                                break;
                            }
                        }
                    }
                    continue;
                }
            };
            firsts.entry(name).or_insert(raw);
        }
        let _ = ws.close(None).await;
        let summary: Vec<String> = counts.iter().map(|(k, v)| format!("{k}={v}")).collect();
        eprintln!(
            "{}: {url}\n  mensajes: {}\n  snapshots pedidos: {snapshots}; delta que engancha: {}",
            prefix(market),
            if summary.is_empty() {
                "ninguno".into()
            } else {
                summary.join(", ")
            },
            if delta.is_some() { "sí" } else { "no" }
        );
        let mut files: Vec<(String, Vec<u8>)> = firsts
            .into_iter()
            .map(|(name, raw)| (format!("{}_{name}", prefix(market)), raw))
            .collect();
        if let (Some(s), Some(d)) = (snapshot, delta) {
            files.push((format!("{}_depth_snapshot", prefix(market)), s));
            files.push((format!("{}_depth", prefix(market)), d));
        }
        if let Some(id) = last_agg_id {
            let raw = get(
                &client,
                &format!(
                    "{rest}{api}/aggTrades?symbol={symbol}&fromId={}&limit=3",
                    id.saturating_sub(2)
                ),
            )
            .await?;
            files.push((format!("{}_rest_aggtrades", prefix(market)), raw));
        }
        if market == Market::UsdM {
            let raw = get(
                &client,
                &format!("{rest}/fapi/v1/openInterest?symbol={symbol}"),
            )
            .await?;
            files.push(("usdm_open_interest".into(), raw));
        }
        for (stem, raw) in &files {
            std::fs::write(out.join(format!("{stem}.json")), compact(raw))
                .map_err(|e| e.to_string())?;
        }
        for (stem, raw) in &files {
            let snap = if stem.ends_with("_depth") {
                std::fs::read(out.join(format!("{stem}_snapshot.json"))).ok()
            } else {
                None
            };
            let expected =
                render::fixture(stem, raw, snap.as_deref()).map_err(|e| format!("{stem}: {e}"))?;
            std::fs::write(out.join(format!("{stem}.esperado")), expected)
                .map_err(|e| e.to_string())?;
            written.push(stem.clone());
        }
    }
    Ok(written)
}
