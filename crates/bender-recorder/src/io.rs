//! Capa de E/S del grabador: una conexión WS por [`Link`] (streams combinados; USDⓈ-M
//! usa dos) con reconexión y backoff, un trabajador REST con límites de tasa y dos
//! prioridades, el tick del reloj y un hilo que pasa todo por el [`Core`] y escribe en el
//! store.
//!
//! Límites de Binance que se respetan:
//! - REST: presupuesto de peso por minuto y por mercado (token bucket, configurable); la
//!   recuperación sólo usa su parte (`recovery_share_percent`) y va después de los
//!   pedidos de prioridad alta (snapshots del libro, open interest). Un 429/418 pausa el
//!   mercado lo que diga `Retry-After`.
//! - WS: una conexión por ruta (spot; USDⓈ-M `/public` y `/market`) con todos los
//!   streams en la URL (sin mensajes de suscripción); reconexión con backoff exponencial
//!   (1 s … 60 s), así nunca se acerca al límite de 300 conexiones cada 5 minutos por IP.

use std::collections::{BTreeMap, VecDeque};
use std::time::Duration;

use bender_store::{Store, WriterOptions};
use bender_types::proto::GapReason;
use bender_venue_binance::Market;
use futures_util::StreamExt;
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

use crate::config::Config;
use crate::core::{Core, Input, Link, Record, Request, Startup};

/// Reloj de pared en ns UTC.
pub fn now_ns() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i64)
        .unwrap_or_default()
}

fn log(msg: &str) {
    eprintln!("[bender-recorder] {msg}");
}

enum CoreMsg {
    Input(Input),
    Stop,
}

/// Peso de un pedido REST según la documentación de Binance.
pub fn weight(market: Market, request: &Request, snapshot_depth: u32) -> u32 {
    match (market, request) {
        (Market::Spot, Request::Snapshot { .. }) => match snapshot_depth {
            0..=100 => 5,
            101..=500 => 25,
            501..=1000 => 50,
            _ => 250,
        },
        (Market::UsdM, Request::Snapshot { .. }) => match snapshot_depth {
            0..=50 => 2,
            51..=100 => 5,
            101..=500 => 10,
            _ => 20,
        },
        (Market::Spot, Request::AggTrades { .. }) => 4,
        (Market::UsdM, Request::AggTrades { .. }) => 20,
        (_, Request::OpenInterest { .. }) => 1,
        (_, Request::Reconnect { .. }) => 0,
    }
}

fn market_of(request: &Request) -> Market {
    match request {
        Request::Snapshot { market, .. } | Request::AggTrades { market, .. } => *market,
        Request::Reconnect { link, .. } => link.market(),
        Request::OpenInterest { .. } => Market::UsdM,
    }
}

/// Token bucket de peso por minuto.
struct Bucket {
    capacity: f64,
    tokens: f64,
    last: tokio::time::Instant,
    paused_until: Option<tokio::time::Instant>,
}

impl Bucket {
    fn new(per_minute: u32) -> Self {
        Self {
            capacity: f64::from(per_minute),
            tokens: f64::from(per_minute),
            last: tokio::time::Instant::now(),
            paused_until: None,
        }
    }

    fn refill(&mut self) {
        let now = tokio::time::Instant::now();
        let dt = now.duration_since(self.last).as_secs_f64();
        self.tokens = (self.tokens + dt * self.capacity / 60.0).min(self.capacity);
        self.last = now;
    }

    /// Cuánto falta para poder gastar `weight` dejando `reserve` sin tocar.
    fn wait_for(&mut self, weight: u32, reserve: f64) -> Duration {
        self.refill();
        let now = tokio::time::Instant::now();
        if let Some(until) = self.paused_until {
            if until > now {
                return until - now;
            }
            self.paused_until = None;
        }
        let need = f64::from(weight) + reserve - self.tokens;
        if need <= 0.0 {
            Duration::ZERO
        } else {
            Duration::from_secs_f64(need * 60.0 / self.capacity)
        }
    }

    fn spend(&mut self, weight: u32) {
        self.tokens -= f64::from(weight);
    }
}

fn url_for(config: &Config, request: &Request) -> Option<String> {
    let market = market_of(request);
    let base = config.endpoints.rest(market).trim_end_matches('/');
    let prefix = match market {
        Market::Spot => "/api/v3",
        Market::UsdM => "/fapi/v1",
    };
    Some(match request {
        Request::Snapshot { symbol, .. } => format!(
            "{base}{prefix}/depth?symbol={symbol}&limit={}",
            config.snapshot_depth
        ),
        Request::AggTrades {
            symbol,
            from_id,
            limit,
            ..
        } => format!("{base}{prefix}/aggTrades?symbol={symbol}&fromId={from_id}&limit={limit}"),
        Request::OpenInterest { symbol } => {
            format!("{base}/fapi/v1/openInterest?symbol={symbol}")
        }
        Request::Reconnect { .. } => return None,
    })
}

enum Fetch {
    Ok(Vec<u8>),
    /// 429/418: esperar esto.
    RateLimited(Duration),
    Failed(String),
}

async fn fetch(client: &reqwest::Client, url: &str) -> Fetch {
    match client.get(url).send().await {
        Ok(resp) => {
            let status = resp.status();
            if status.as_u16() == 429 || status.as_u16() == 418 {
                let secs = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(60);
                return Fetch::RateLimited(Duration::from_secs(secs));
            }
            if !status.is_success() {
                return Fetch::Failed(format!("HTTP {status}"));
            }
            match resp.bytes().await {
                Ok(b) => Fetch::Ok(b.to_vec()),
                Err(e) => Fetch::Failed(e.to_string()),
            }
        }
        Err(e) => Fetch::Failed(e.to_string()),
    }
}

/// Trabajador REST: prioridad alta (snapshots, open interest) antes que la baja
/// (recuperación), con presupuesto de peso por mercado.
async fn rest_worker(
    config: Config,
    mut rx: mpsc::UnboundedReceiver<Request>,
    tx: mpsc::UnboundedSender<CoreMsg>,
    mut stop: watch::Receiver<bool>,
) {
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .build()
    {
        Ok(c) => c,
        Err(e) => {
            log(&format!("no se pudo crear el cliente HTTP: {e}"));
            return;
        }
    };
    let mut buckets: BTreeMap<Market, Bucket> = BTreeMap::from([
        (
            Market::Spot,
            Bucket::new(config.rate_limits.spot_weight_per_minute),
        ),
        (
            Market::UsdM,
            Bucket::new(config.rate_limits.usdm_weight_per_minute),
        ),
    ]);
    let share = f64::from(config.rate_limits.recovery_share_percent.min(100)) / 100.0;
    let mut high: VecDeque<(Request, u32)> = VecDeque::new();
    let mut low: VecDeque<(Request, u32)> = VecDeque::new();
    loop {
        while let Ok(r) = rx.try_recv() {
            match r {
                Request::AggTrades { .. } => low.push_back((r, 0)),
                _ => high.push_back((r, 0)),
            }
        }
        if high.is_empty() && low.is_empty() {
            tokio::select! {
                r = rx.recv() => match r {
                    Some(r @ Request::AggTrades { .. }) => low.push_back((r, 0)),
                    Some(r) => high.push_back((r, 0)),
                    None => return,
                },
                _ = stop.changed() => return,
            }
            continue;
        }
        let from_high = !high.is_empty();
        let (request, attempt) = if from_high {
            high.pop_front()
        } else {
            low.pop_front()
        }
        .expect("cola no vacía");
        let market = market_of(&request);
        let w = weight(market, &request, config.snapshot_depth);
        let bucket = buckets.get_mut(&market).expect("bucket");
        let reserve = if from_high {
            0.0
        } else {
            bucket.capacity * (1.0 - share)
        };
        let wait = bucket.wait_for(w, reserve);
        if !wait.is_zero() {
            // Esperar sin perder pedidos nuevos de prioridad alta.
            if from_high {
                high.push_front((request, attempt));
            } else {
                low.push_front((request, attempt));
            }
            tokio::select! {
                _ = tokio::time::sleep(wait.min(Duration::from_millis(250))) => {},
                _ = stop.changed() => return,
            }
            continue;
        }
        bucket.spend(w);
        let Some(url) = url_for(&config, &request) else {
            continue;
        };
        let result = tokio::select! {
            r = fetch(&client, &url) => r,
            _ = stop.changed() => return,
        };
        let ts = now_ns();
        match result {
            Fetch::Ok(raw) => {
                let input = match request {
                    Request::Snapshot { market, symbol } => Input::Snapshot {
                        market,
                        symbol,
                        ts,
                        raw,
                    },
                    Request::AggTrades {
                        market,
                        symbol,
                        from_id,
                        ..
                    } => Input::AggTrades {
                        market,
                        symbol,
                        from_id,
                        ts,
                        raw,
                    },
                    Request::OpenInterest { symbol } => Input::OpenInterest { symbol, ts, raw },
                    Request::Reconnect { .. } => continue,
                };
                if tx.send(CoreMsg::Input(input)).is_err() {
                    return;
                }
            }
            Fetch::RateLimited(d) => {
                log(&format!("límite de tasa en {url}: pausa de {d:?}"));
                buckets.get_mut(&market).expect("bucket").paused_until =
                    Some(tokio::time::Instant::now() + d);
                if from_high {
                    high.push_front((request, attempt));
                } else {
                    low.push_front((request, attempt));
                }
            }
            Fetch::Failed(e) => {
                log(&format!("falló {url} (intento {}): {e}", attempt + 1));
                match request {
                    Request::AggTrades {
                        market,
                        symbol,
                        from_id,
                        ..
                    } if attempt + 1 >= 5 => {
                        let _ = tx.send(CoreMsg::Input(Input::AggTradesFailed {
                            market,
                            symbol,
                            from_id,
                            ts,
                            detail: e,
                        }));
                    }
                    Request::OpenInterest { .. } => {}
                    r => {
                        let backoff = Duration::from_millis(500 << attempt.min(6));
                        tokio::select! {
                            _ = tokio::time::sleep(backoff) => {},
                            _ = stop.changed() => return,
                        }
                        if from_high {
                            high.push_back((r, attempt + 1));
                        } else {
                            low.push_front((r, attempt + 1));
                        }
                    }
                }
            }
        }
    }
}

/// Una conexión WS, con reconexión.
async fn ws_task(
    link: Link,
    url: String,
    min_backoff: Duration,
    max_backoff: Duration,
    tx: mpsc::UnboundedSender<CoreMsg>,
    mut cmd: mpsc::UnboundedReceiver<(GapReason, String)>,
    mut stop: watch::Receiver<bool>,
) {
    let mut backoff = min_backoff;
    loop {
        if *stop.borrow() {
            return;
        }
        let connected = tokio::select! {
            r = tokio_tungstenite::connect_async(url.as_str()) => r,
            _ = stop.changed() => return,
        };
        match connected {
            Ok((mut ws, _)) => {
                let since = tokio::time::Instant::now();
                let _ = tx.send(CoreMsg::Input(Input::Connected { link, ts: now_ns() }));
                let (reason, detail) = loop {
                    tokio::select! {
                        msg = ws.next() => match msg {
                            Some(Ok(Message::Text(t))) => {
                                let _ = tx.send(CoreMsg::Input(Input::Message {
                                    link,
                                    ts: now_ns(),
                                    raw: t.as_bytes().to_vec(),
                                }));
                            }
                            Some(Ok(Message::Binary(b))) => {
                                let _ = tx.send(CoreMsg::Input(Input::Message {
                                    link,
                                    ts: now_ns(),
                                    raw: b.to_vec(),
                                }));
                            }
                            Some(Ok(Message::Close(frame))) => {
                                break (GapReason::Disconnected, format!("cierre del servidor: {frame:?}"));
                            }
                            Some(Ok(_)) => {} // ping/pong: tungstenite contesta solo
                            Some(Err(e)) => break (GapReason::Disconnected, e.to_string()),
                            None => break (GapReason::Disconnected, "conexión terminada".into()),
                        },
                        c = cmd.recv() => {
                            let (reason, detail) = c.unwrap_or((GapReason::Disconnected, "cerrada".into()));
                            let _ = ws.close(None).await;
                            break (reason, detail);
                        }
                        _ = stop.changed() => {
                            let _ = ws.close(None).await;
                            return;
                        }
                    }
                };
                log(&format!("{link:?}: desconectado ({detail})"));
                let _ = tx.send(CoreMsg::Input(Input::Disconnected {
                    link,
                    ts: now_ns(),
                    reason,
                    detail,
                }));
                if since.elapsed() > max_backoff {
                    backoff = min_backoff;
                }
            }
            Err(e) => log(&format!("{link:?}: no se pudo conectar: {e}")),
        }
        tokio::select! {
            _ = tokio::time::sleep(backoff) => {},
            _ = stop.changed() => return,
        }
        backoff = (backoff * 2).min(max_backoff);
    }
}

/// URL del WS combinado de una conexión.
pub fn ws_url(config: &Config, link: Link) -> String {
    format!(
        "{}/stream?streams={}",
        config.endpoints.ws(link).trim_end_matches('/'),
        config.streams(link).join("/")
    )
}

/// Corre el grabador hasta que `shutdown` termina. Devuelve el store con todo cerrado.
pub async fn run(
    config: Config,
    store: Store,
    shutdown: impl std::future::Future<Output = ()>,
) -> Result<Store, String> {
    let startup = Startup::from_store(&store).map_err(|e| e.to_string())?;
    let core = Core::new(config.core(), startup);
    let markets = core.config().markets();
    let (core_tx, mut core_rx) = mpsc::unbounded_channel::<CoreMsg>();
    let (rest_tx, rest_rx) = mpsc::unbounded_channel::<Request>();
    let (stop_tx, stop_rx) = watch::channel(false);
    let mut ws_cmds = BTreeMap::new();
    let mut tasks = Vec::new();
    for link in markets.iter().flat_map(|m| Link::of(*m).iter().copied()) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        ws_cmds.insert(link, cmd_tx);
        tasks.push(tokio::spawn(ws_task(
            link,
            ws_url(&config, link),
            Duration::from_millis(config.reconnect_min_ms),
            Duration::from_millis(config.reconnect_max_ms),
            core_tx.clone(),
            cmd_rx,
            stop_rx.clone(),
        )));
    }
    tasks.push(tokio::spawn(rest_worker(
        config.clone(),
        rest_rx,
        core_tx.clone(),
        stop_rx.clone(),
    )));
    {
        let tx = core_tx.clone();
        let mut stop = stop_rx.clone();
        let every = Duration::from_millis(config.heartbeat_ms);
        tasks.push(tokio::spawn(async move {
            let mut interval = tokio::time::interval(every);
            interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = interval.tick() => {
                        if tx.send(CoreMsg::Input(Input::Tick { ts: now_ns() })).is_err() {
                            return;
                        }
                    }
                    _ = stop.changed() => return,
                }
            }
        }));
    }

    // Hilo del núcleo: aplica cada entrada y escribe en el store.
    let options = WriterOptions {
        max_rows: config.segment_rows,
        max_bytes: config.segment_mib * 1024 * 1024,
    };
    let core_thread = std::thread::spawn(move || -> Result<Store, String> {
        let mut core = core;
        let mut store = store;
        let mut error = None;
        while let Some(msg) = core_rx.blocking_recv() {
            let input = match msg {
                CoreMsg::Input(i) => i,
                CoreMsg::Stop => break,
            };
            if error.is_some() {
                continue;
            }
            let out = core.handle(input);
            for r in &out.records {
                let res = match r {
                    Record::Normalized { source, envelope } => store.append_envelope(
                        source,
                        bender_venue_binance::NORMALIZER_VERSION,
                        &options,
                        envelope,
                    ),
                    Record::Raw { source, record } => store.append_raw(source, &options, record),
                };
                if let Err(e) = res {
                    log(&format!("error del store: {e}"));
                    error = Some(e.to_string());
                    break;
                }
            }
            for req in out.requests {
                match req {
                    Request::Reconnect {
                        link,
                        reason,
                        detail,
                    } => {
                        if let Some(c) = ws_cmds.get(&link) {
                            let _ = c.send((reason, detail));
                        }
                    }
                    r => {
                        let _ = rest_tx.send(r);
                    }
                }
            }
        }
        store.close_all().map_err(|e| e.to_string())?;
        match error {
            Some(e) => Err(e),
            None => Ok(store),
        }
    });

    shutdown.await;
    log("apagando: cerrando conexiones y segmentos");
    let _ = stop_tx.send(true);
    for t in tasks {
        let _ = t.await;
    }
    let _ = core_tx.send(CoreMsg::Stop);
    tokio::task::spawn_blocking(move || core_thread.join())
        .await
        .map_err(|e| e.to_string())?
        .map_err(|_| "el hilo del núcleo terminó con pánico".to_string())?
}
