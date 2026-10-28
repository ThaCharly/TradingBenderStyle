//! Grabador completo contra un Binance simulado (WS + REST locales): conexión, corte,
//! reconexión con recuperación por REST, libro, open interest y latidos. Después, los
//! segmentos grabados se leen con `StoreSource` y producen velas con el Runner.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use axum::Router;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use axum::routing::get;
use bender_kernel::KernelConfig;
use bender_recorder::config::{Config, Endpoints, InstrumentConfig, MarketName};
use bender_recorder::core::BookMode;
use bender_runner::normative::BarsConsumer;
use bender_runner::{StoreSource, backtest};
use bender_store::Store;
use bender_types::proto::{self, Provenance, RecoveryResult, envelope};
use serde::Deserialize;

#[derive(Default)]
struct Mock {
    /// Conexiones a `/market` (trades) y a `/public` (libro).
    connections: AtomicUsize,
    public_connections: AtomicUsize,
    agg_requests: AtomicUsize,
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

/// Trades de prueba: id → instante fijo relativo al inicio.
fn trade_json(id: u64, t: i64) -> String {
    format!(
        r#"{{"a":{id},"p":"{}.50","q":"0.100","f":{id},"l":{id},"T":{t},"m":{}}}"#,
        100_000 + id % 13,
        id.is_multiple_of(2)
    )
}

async fn market_ws(ws: WebSocketUpgrade, State(mock): State<Arc<Mock>>) -> impl IntoResponse {
    let n = mock.connections.fetch_add(1, Ordering::SeqCst);
    ws.on_upgrade(move |socket| serve(socket, n, false))
}

async fn public_ws(ws: WebSocketUpgrade, State(mock): State<Arc<Mock>>) -> impl IntoResponse {
    let n = mock.public_connections.fetch_add(1, Ordering::SeqCst);
    ws.on_upgrade(move |socket| serve(socket, n, true))
}

/// Como Binance: `/market` manda los trades y `/public` el libro. Las dos conexiones se
/// cortan a la vez (se cae la red) después de 50 mensajes.
async fn serve(mut socket: WebSocket, n: usize, public: bool) {
    let (ids, depth_start) = if n == 0 {
        (1..=50u64, 1000u64)
    } else {
        (101..=150, 5000)
    };
    let mut u = depth_start;
    for id in ids {
        let t = now_ms();
        let trade = trade_json(id, t).replacen(
            '{',
            &format!(r#"{{"e":"aggTrade","E":{t},"s":"BTCUSDT","#),
            1,
        );
        let msg = format!(r#"{{"stream":"btcusdt@aggTrade","data":{trade}}}"#);
        if !public && socket.send(Message::Text(msg.into())).await.is_err() {
            return;
        }
        let depth = format!(
            r#"{{"stream":"btcusdt@depth@100ms","data":{{"e":"depthUpdate","E":{t},"T":{t},"s":"BTCUSDT","U":{},"u":{},"pu":{u},"b":[["{}.00","1.000"]],"a":[]}}}}"#,
            u + 1,
            u + 2,
            99_000 + id % 7
        );
        u += 2;
        if public && socket.send(Message::Text(depth.into())).await.is_err() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if n == 0 {
        // Corte: el servidor cierra la conexión.
        let _ = socket.send(Message::Close(None)).await;
        return;
    }
    // Segunda conexión: queda abierta hasta que el cliente se va.
    while let Some(Ok(_)) = socket.recv().await {}
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AggQuery {
    from_id: u64,
    limit: usize,
}

async fn agg_trades(Query(q): Query<AggQuery>, State(mock): State<Arc<Mock>>) -> String {
    mock.agg_requests.fetch_add(1, Ordering::SeqCst);
    let base = now_ms() - 2_000;
    let trades: Vec<String> = (q.from_id..=200)
        .take(q.limit)
        .map(|id| trade_json(id, base + id as i64))
        .collect();
    format!("[{}]", trades.join(","))
}

#[derive(Deserialize)]
struct DepthQuery {}

async fn depth(Query(_): Query<DepthQuery>, State(mock): State<Arc<Mock>>) -> String {
    // Un snapshot que engancha con la conexión actual.
    let id = if mock.public_connections.load(Ordering::SeqCst) <= 1 {
        1000
    } else {
        5000
    };
    format!(
        r#"{{"lastUpdateId":{id},"E":{t},"T":{t},"bids":[["99000.00","2.000"],["98999.00","1.000"]],"asks":[["99001.00","1.000"]]}}"#,
        t = now_ms()
    )
}

async fn open_interest() -> String {
    format!(
        r#"{{"openInterest":"12345.678","symbol":"BTCUSDT","time":{}}}"#,
        now_ms()
    )
}

fn temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("bender-recorder-io-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn records_recovers_and_replays_into_bars() {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let dir = temp_dir("e2e");
    let mock = Arc::new(Mock::default());
    let store = rt.block_on(async {
        let app = Router::new()
            .route("/market/stream", get(market_ws))
            .route("/public/stream", get(public_ws))
            .route("/fapi/v1/aggTrades", get(agg_trades))
            .route("/fapi/v1/depth", get(depth))
            .route("/fapi/v1/openInterest", get(open_interest))
            .with_state(mock.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let config = Config {
            instruments: vec![InstrumentConfig {
                market: MarketName::Usdm,
                symbol: "BTCUSDT".into(),
                book: BookMode::L2,
            }],
            reconnect_min_ms: 50,
            reconnect_max_ms: 200,
            endpoints: Endpoints {
                spot_ws: format!("ws://{addr}"),
                spot_rest: format!("http://{addr}"),
                usdm_ws_public: format!("ws://{addr}/public"),
                usdm_ws_market: format!("ws://{addr}/market"),
                usdm_rest: format!("http://{addr}"),
            },
            ..Config::default()
        };
        let store = Store::open(&dir).unwrap();
        // Dos conexiones de 50 trades cada ~20 ms más el corte: unos 2,5 s.
        bender_recorder::io::run(
            config,
            store,
            tokio::time::sleep(Duration::from_millis(3_500)),
        )
        .await
        .unwrap()
    });
    assert_eq!(
        mock.connections.load(Ordering::SeqCst),
        2,
        "/market reconectó una vez"
    );
    assert_eq!(
        mock.public_connections.load(Ordering::SeqCst),
        2,
        "/public reconectó una vez"
    );
    assert!(
        mock.agg_requests.load(Ordering::SeqCst) >= 1,
        "pidió los trades perdidos"
    );

    let trades_key = "BINANCE:PERP_LINEAR:BTCUSDT:trades";
    let events = store
        .read_normalized(trades_key, i64::MIN, i64::MAX)
        .unwrap();
    let trades: Vec<&proto::Envelope> = events
        .iter()
        .filter(|e| matches!(&e.payload, Some(envelope::Payload::Market(m)) if matches!(m.event, Some(proto::market_event::Event::Trade(_)))))
        .collect();
    let ids: Vec<u64> = trades.iter().map(|e| e.source_seq.unwrap()).collect();
    let unique: BTreeSet<u64> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len(), "sin duplicados");
    assert_eq!(unique, (1..=150).collect(), "todos los trades");
    for e in &trades {
        let id = e.source_seq.unwrap();
        let expected = if (51..=100).contains(&id) {
            Provenance::Recovered
        } else {
            Provenance::Live
        };
        assert_eq!(e.provenance, expected as i32, "trade {id}");
    }
    let gaps = events
        .iter()
        .filter(|e| matches!(e.payload, Some(envelope::Payload::Gap(_))))
        .count();
    assert_eq!(gaps, 1);
    let full = events
        .iter()
        .filter(|e| matches!(&e.payload, Some(envelope::Payload::GapRecovery(r)) if r.result == RecoveryResult::Full as i32))
        .count();
    assert_eq!(full, 1);

    // Libro: snapshots y un hueco por la desconexión (resincronizado).
    let book = store
        .read_normalized("BINANCE:PERP_LINEAR:BTCUSDT:book", i64::MIN, i64::MAX)
        .unwrap();
    assert!(book.iter().filter(|e| matches!(&e.payload, Some(envelope::Payload::Market(m)) if matches!(m.event, Some(proto::market_event::Event::BookSnapshot(_))))).count() >= 2);
    assert!(
        book.iter()
            .any(|e| matches!(e.payload, Some(envelope::Payload::Gap(_))))
    );
    assert!(book.iter().any(|e| matches!(&e.payload, Some(envelope::Payload::Market(m)) if matches!(m.event, Some(proto::market_event::Event::BookDelta(_))))));
    // Open interest, latidos y zona cruda.
    assert!(
        !store
            .read_normalized(
                "BINANCE:PERP_LINEAR:BTCUSDT:open_interest",
                i64::MIN,
                i64::MAX
            )
            .unwrap()
            .is_empty()
    );
    let beats = store.read_normalized("CLOCK", i64::MIN, i64::MAX).unwrap();
    assert!(beats.len() >= 20, "latidos cada 100 ms: {}", beats.len());
    assert!(
        !store
            .read_raw(
                "BINANCE:PERP_LINEAR:BTCUSDT:ws:aggTrade",
                i64::MIN,
                i64::MAX
            )
            .unwrap()
            .is_empty()
    );
    assert!(
        !store
            .read_raw(
                "BINANCE:PERP_LINEAR:BTCUSDT:rest:aggTrades",
                i64::MIN,
                i64::MAX
            )
            .unwrap()
            .is_empty()
    );

    // Los segmentos se leen con StoreSource y producen velas con el Runner.
    let sources: Vec<String> = store
        .segments()
        .iter()
        .filter(|s| s.zone == bender_store::Zone::Normalized)
        .map(|s| s.source.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut source = StoreSource::open(&store, &sources, i64::MIN, i64::MAX).unwrap();
    let result = backtest(KernelConfig::default(), BarsConsumer, &mut source).unwrap();
    let bars = result
        .outputs
        .iter()
        .filter(|o| matches!(o, bender_kernel::KernelOutput::Bar(b) if &*b.source == trades_key))
        .count();
    assert!(
        bars > 0,
        "el Runner cerró velas de 1 s con los latidos grabados"
    );

    // Reporte: tamaños por tipo de dato, huecos y recuperaciones.
    let report = bender_recorder::report::Report::from_store(&store).unwrap();
    let sizes = report.sizes();
    use bender_recorder::report::Category;
    use bender_store::Zone;
    for key in [
        (Zone::Raw, Category::Trades),
        (Zone::Normalized, Category::Trades),
        (Zone::Raw, Category::L2),
        (Zone::Normalized, Category::L2),
        (Zone::Normalized, Category::Other),
    ] {
        assert!(
            sizes
                .get(&key)
                .is_some_and(|(rows, bytes)| *rows > 0 && *bytes > 0),
            "{key:?}"
        );
    }
    assert_eq!(
        report.gap_reasons.get("disconnected"),
        Some(&6),
        "trades, libro, mark, index, funding, liquidaciones"
    );
    let md = report.markdown();
    assert!(
        md.contains("| normalized | trades |") && md.contains("full: 1"),
        "{md}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}
