//! Servicio de ejemplo (`Echo` + `StreamCounter`), con inyección de cortes para los tests
//! de reanudación.

use std::pin::Pin;
use std::sync::{Arc, Mutex};

use tokio::sync::mpsc;
use tokio_stream::Stream;
use tokio_stream::wrappers::ReceiverStream;
use tonic::service::Routes;
use tonic::{Request, Response, Status};

use crate::proto::example::example_service_server::{ExampleService, ExampleServiceServer};
use crate::proto::example::{
    EchoRequest, EchoResponse, StreamCounterRequest, StreamCounterResponse,
};
use crate::version::{ServiceVersions, Version};

/// Nombre completo del servicio.
pub const SERVICE: &str = "bender.example.v1.ExampleService";
/// Versión de protocolo del servicio de ejemplo.
pub const VERSION: Version = Version::new(1, 0);

/// Generador pseudoaleatorio determinista (SplitMix64), para cortes reproducibles.
#[derive(Clone, Debug)]
pub struct SplitMix64(u64);

impl SplitMix64 {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }

    /// Verdadero con probabilidad `per_mille / 1000`.
    pub fn chance(&mut self, per_mille: u64) -> bool {
        self.next_u64() % 1000 < per_mille
    }
}

/// Cortes simulados del lado del servidor: antes de emitir cada evento, con probabilidad
/// `per_mille / 1000`, el stream termina con `UNAVAILABLE`.
#[derive(Debug)]
pub struct Faults {
    rng: SplitMix64,
    per_mille: u64,
    cuts: u64,
}

impl Faults {
    pub fn new(seed: u64, per_mille: u64) -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(Self {
            rng: SplitMix64::new(seed),
            per_mille,
            cuts: 0,
        }))
    }

    pub fn cuts(&self) -> u64 {
        self.cuts
    }

    fn should_cut(&mut self) -> bool {
        let cut = self.rng.chance(self.per_mille);
        self.cuts += u64::from(cut);
        cut
    }
}

/// Implementación del servicio de ejemplo.
#[derive(Clone, Debug, Default)]
pub struct Example {
    faults: Option<Arc<Mutex<Faults>>>,
}

impl Example {
    pub fn with_faults(faults: Arc<Mutex<Faults>>) -> Self {
        Self {
            faults: Some(faults),
        }
    }

    /// Rutas y versiones listas para [`crate::server::serve`] o
    /// [`crate::Channel::in_process`].
    pub fn routes(self) -> (Routes, ServiceVersions) {
        (
            Routes::new(ExampleServiceServer::new(self)),
            ServiceVersions::new([(SERVICE, VERSION)]),
        )
    }
}

type CounterStream = Pin<Box<dyn Stream<Item = Result<StreamCounterResponse, Status>> + Send>>;

#[tonic::async_trait]
impl ExampleService for Example {
    async fn echo(&self, req: Request<EchoRequest>) -> Result<Response<EchoResponse>, Status> {
        Ok(Response::new(EchoResponse {
            text: req.into_inner().text,
        }))
    }

    type StreamCounterStream = CounterStream;

    async fn stream_counter(
        &self,
        req: Request<StreamCounterRequest>,
    ) -> Result<Response<CounterStream>, Status> {
        let req = req.into_inner();
        let first = req.after_ingest_seq.map_or(1, |c| c + 1);
        let faults = self.faults.clone();
        let (tx, rx) = mpsc::channel(32);
        tokio::spawn(async move {
            for seq in first..=req.until_ingest_seq {
                let cut = faults
                    .as_ref()
                    .is_some_and(|f| f.lock().expect("faults").should_cut());
                if cut {
                    let _ = tx.send(Err(Status::unavailable("corte simulado"))).await;
                    return;
                }
                let event = StreamCounterResponse {
                    ingest_seq: seq,
                    value: seq.wrapping_mul(seq),
                };
                if tx.send(Ok(event)).await.is_err() {
                    return;
                }
            }
        });
        Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
    }
}
