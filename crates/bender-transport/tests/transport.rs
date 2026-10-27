//! Suite de #21: los mismos tests corren con el transporte en proceso y con el de red.

use std::net::SocketAddr;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bender_transport::example::{self, Example, Faults, SplitMix64};
use bender_transport::proto::example::example_service_client::ExampleServiceClient;
use bender_transport::proto::example::{EchoRequest, StreamCounterRequest, StreamCounterResponse};
use bender_transport::resume::{ResumePolicy, resumable};
use bender_transport::server::{ServerTls, serve};
use bender_transport::version::{self, Compatibility};
use bender_transport::{Channel, ClientTls, TransportError, Version};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio_stream::StreamExt;

/// Dónde corre el servicio de ejemplo y cómo se le habla.
enum Harness {
    InProcess(Example),
    Network { url: String, tls: Option<ClientTls> },
}

impl Harness {
    fn in_process(example: Example) -> Self {
        Self::InProcess(example)
    }

    async fn network(example: Example) -> Self {
        let addr = spawn_server(example, None).await;
        Self::Network {
            url: format!("http://{addr}"),
            tls: None,
        }
    }

    fn client(&self, version: Version) -> ExampleServiceClient<Channel> {
        let channel = match self {
            Self::InProcess(example) => {
                let (routes, versions) = example.clone().routes();
                Channel::in_process(routes, versions, version)
            }
            Self::Network { url, tls } => Channel::connect(url, version, tls.clone()).unwrap(),
        };
        ExampleServiceClient::new(channel)
    }
}

async fn spawn_server(example: Example, tls: Option<ServerTls>) -> SocketAddr {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (routes, versions) = example.routes();
    tokio::spawn(serve(
        listener,
        routes,
        versions,
        tls,
        std::future::pending(),
    ));
    addr
}

// ---------------------------------------------------------------------------------------
// La suite: cada función recibe el harness y se instancia para los dos transportes.

async fn echo(h: Harness) {
    let mut client = h.client(example::VERSION);
    let response = client
        .echo(EchoRequest {
            text: "hola Bender".into(),
        })
        .await
        .unwrap();
    assert_eq!(
        version::from_metadata(response.metadata()),
        Some(example::VERSION),
        "la respuesta trae la versión del servicio"
    );
    assert_eq!(response.into_inner().text, "hola Bender");
}

async fn stream_counter(h: Harness) {
    let mut client = h.client(example::VERSION);
    let request = StreamCounterRequest {
        after_ingest_seq: Some(10),
        until_ingest_seq: 25,
    };
    let events: Vec<_> = client
        .stream_counter(request)
        .await
        .unwrap()
        .into_inner()
        .map(|e| e.unwrap().ingest_seq)
        .collect()
        .await;
    assert_eq!(events, (11..=25).collect::<Vec<_>>());
}

/// Criterio de #21: cortes en eventos aleatorios; al reconectar con el cursor el cliente
/// recibe N+1… sin huecos ni duplicados. 1.000 cortes como mínimo.
async fn resumes_after_1000_random_cuts(seed: u64, make: impl AsyncFnOnce(Example) -> Harness) {
    const UNTIL: u64 = 20_000;
    let faults = Faults::new(seed, 60); // ~6 % por evento → ~1.200 cortes
    let h = make(Example::with_faults(faults.clone())).await;
    let client = h.client(example::VERSION);
    let policy = ResumePolicy {
        backoff: Duration::ZERO,
        max_attempts_without_progress: 50,
    };
    let (stream, stats) = resumable(None, policy, move |after| {
        let mut client = client.clone();
        async move {
            let request = StreamCounterRequest {
                after_ingest_seq: after,
                until_ingest_seq: UNTIL,
            };
            Ok(client.stream_counter(request).await?.into_inner())
        }
    });
    let events: Vec<StreamCounterResponse> = stream.map(Result::unwrap).collect().await;

    let cuts = faults.lock().unwrap().cuts();
    assert!(cuts >= 1_000, "sólo hubo {cuts} cortes");
    assert_eq!(events.len() as u64, UNTIL, "sin huecos ni duplicados");
    for (i, e) in events.iter().enumerate() {
        let expected = i as u64 + 1;
        assert_eq!(e.ingest_seq, expected);
        assert_eq!(e.value, expected * expected);
    }
    assert_eq!(stats.borrow().reconnects, cuts);
}

/// Criterio de #21: un cliente de versión mayor incompatible recibe un error tipado.
async fn incompatible_version_is_typed(h: Harness) {
    for client_version in [Version::new(2, 0), Version::new(1, 2), Version::new(0, 9)] {
        let mut client = h.client(client_version);
        let err = client
            .echo(EchoRequest { text: "x".into() })
            .await
            .map_err(TransportError::from)
            .unwrap_err();
        let TransportError::IncompatibleVersion(mismatch) = err else {
            panic!("error no tipado para {client_version}: {err}");
        };
        assert_eq!(mismatch.service, example::SERVICE);
        assert_eq!(mismatch.server.map(Version::from), Some(example::VERSION));
        assert_eq!(mismatch.client.map(Version::from), Some(client_version));
        assert!(!TransportError::IncompatibleVersion(mismatch).is_retryable());
    }

    // Una menor de diferencia se acepta; el cliente ve que está desactualizado.
    let mut client = h.client(Version::new(1, 1));
    let response = client.echo(EchoRequest { text: "y".into() }).await.unwrap();
    let server = version::from_metadata(response.metadata()).unwrap();
    assert_eq!(
        version::compatibility(server, Version::new(1, 1)),
        Compatibility::MinorSkew
    );
}

/// Los streams también se rechazan con error tipado y el reanudador no reintenta.
async fn incompatible_stream_is_not_retried(h: Harness) {
    let client = h.client(Version::new(3, 0));
    let (stream, stats) = resumable(None, ResumePolicy::default(), move |after| {
        let mut client = client.clone();
        async move {
            let request = StreamCounterRequest {
                after_ingest_seq: after,
                until_ingest_seq: 5,
            };
            Ok(client.stream_counter(request).await?.into_inner())
        }
    });
    let results: Vec<_> = stream.collect().await;
    assert_eq!(results.len(), 1);
    assert!(matches!(
        results[0],
        Err(TransportError::IncompatibleVersion(_))
    ));
    assert_eq!(stats.borrow().reconnects, 0);
}

macro_rules! both_transports {
    ($($name:ident),* $(,)?) => {
        mod in_process {
            use super::*;
            $(#[tokio::test]
            async fn $name() {
                super::$name(Harness::in_process(Example::default())).await;
            })*
        }
        mod network {
            use super::*;
            $(#[tokio::test]
            async fn $name() {
                super::$name(Harness::network(Example::default()).await).await;
            })*
        }
    };
}

both_transports!(
    echo,
    stream_counter,
    incompatible_version_is_typed,
    incompatible_stream_is_not_retried,
);

#[tokio::test]
async fn in_process_resumes_after_1000_random_cuts() {
    resumes_after_1000_random_cuts(7, async |e| Harness::in_process(e)).await;
}

#[tokio::test]
async fn network_resumes_after_1000_random_cuts() {
    resumes_after_1000_random_cuts(7, Harness::network).await;
}

// ---------------------------------------------------------------------------------------
// Sólo red.

/// Proxy TCP que corta cada conexión después de una cantidad aleatoria de bytes del
/// servidor: un corte de red de verdad, no un error ordenado del servicio.
async fn spawn_cutting_proxy(upstream: SocketAddr, seed: u64) -> (SocketAddr, Arc<Mutex<u64>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let cuts = Arc::new(Mutex::new(0u64));
    let counter = cuts.clone();
    tokio::spawn(async move {
        let mut rng = SplitMix64::new(seed);
        loop {
            let (mut inbound, _) = listener.accept().await.unwrap();
            let budget = 500 + (rng.next_u64() % 4_000) as usize;
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut outbound = TcpStream::connect(upstream).await.unwrap();
                let (mut ri, mut wi) = inbound.split();
                let (mut ro, mut wo) = outbound.split();
                let up = tokio::io::copy(&mut ri, &mut wo);
                let down = async {
                    let mut sent = 0;
                    let mut buf = [0u8; 512];
                    loop {
                        let n = ro.read(&mut buf).await.unwrap_or(0);
                        if n == 0 {
                            return false;
                        }
                        let n = n.min(budget - sent);
                        if wi.write_all(&buf[..n]).await.is_err() {
                            return false;
                        }
                        sent += n;
                        if sent >= budget {
                            return true;
                        }
                    }
                };
                let cut = tokio::select! {
                    _ = up => false,
                    cut = down => cut,
                };
                if cut {
                    *counter.lock().unwrap() += 1;
                }
                // Al salir se cierran las dos puntas: el cliente ve la conexión caída.
            });
        }
    });
    (addr, cuts)
}

#[tokio::test]
async fn network_resumes_after_real_connection_cuts() {
    const UNTIL: u64 = 20_000;
    let server = spawn_server(Example::default(), None).await;
    let (proxy, cuts) = spawn_cutting_proxy(server, 11).await;
    let channel = Channel::connect(&format!("http://{proxy}"), example::VERSION, None).unwrap();
    let client = ExampleServiceClient::new(channel);
    let policy = ResumePolicy {
        backoff: Duration::from_millis(1),
        max_attempts_without_progress: 50,
    };
    let (stream, stats) = resumable(None, policy, move |after| {
        let mut client = client.clone();
        async move {
            let request = StreamCounterRequest {
                after_ingest_seq: after,
                until_ingest_seq: UNTIL,
            };
            Ok(client.stream_counter(request).await?.into_inner())
        }
    });
    let events: Vec<u64> = stream.map(|e| e.unwrap().ingest_seq).collect().await;
    assert_eq!(events, (1..=UNTIL).collect::<Vec<_>>());
    let cuts = *cuts.lock().unwrap();
    assert!(cuts >= 20, "sólo hubo {cuts} cortes de conexión");
    assert!(stats.borrow().reconnects >= 1);
}

#[tokio::test]
async fn network_with_tls() {
    let cert = rcgen::generate_simple_self_signed(vec!["localhost".into()]).unwrap();
    let cert_pem = cert.cert.pem().into_bytes();
    let tls = ServerTls {
        cert_pem: cert_pem.clone(),
        key_pem: cert.signing_key.serialize_pem().into_bytes(),
    };
    let addr = spawn_server(Example::default(), Some(tls)).await;
    let h = Harness::Network {
        url: format!("https://localhost:{}", addr.port()),
        tls: Some(ClientTls {
            ca_pem: cert_pem,
            domain: "localhost".into(),
        }),
    };
    echo(h).await;

    // Sin TLS contra un servidor con TLS, la llamada falla (no se degrada a texto plano).
    let mut plain = Harness::Network {
        url: format!("http://localhost:{}", addr.port()),
        tls: None,
    }
    .client(example::VERSION);
    assert!(plain.echo(EchoRequest { text: "z".into() }).await.is_err());
}
