//! Streams reanudables por cursor (doc 07 §2.2): ante un corte, se vuelve a abrir el
//! stream desde el último cursor recibido. El resultado no tiene huecos ni duplicados
//! siempre que el servidor emita desde `after + 1`; además se descartan los eventos con
//! cursor ≤ al último entregado, por si un servidor reenvía.

use std::future::Future;
use std::time::Duration;

use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::{Stream, StreamExt};
use tonic::Status;

use crate::TransportError;

/// Evento con cursor (`ingest_seq`).
pub trait Cursor {
    fn cursor(&self) -> u64;
}

impl Cursor for crate::proto::example::StreamCounterResponse {
    fn cursor(&self) -> u64 {
        self.ingest_seq
    }
}

/// Política de reconexión.
#[derive(Clone, Debug)]
pub struct ResumePolicy {
    /// Espera antes de reconectar.
    pub backoff: Duration,
    /// Reconexiones seguidas sin recibir ningún evento antes de rendirse.
    pub max_attempts_without_progress: u32,
}

impl Default for ResumePolicy {
    fn default() -> Self {
        Self {
            backoff: Duration::from_millis(200),
            max_attempts_without_progress: 10,
        }
    }
}

/// Estadísticas de un stream reanudable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ResumeStats {
    pub reconnects: u64,
    pub duplicates_dropped: u64,
}

/// Abre un stream con `open(cursor)` y lo reabre desde el último cursor después de cada
/// corte reintentable. Termina cuando el servidor cierra el stream sin error, ante un
/// error no reintentable (que se entrega como último elemento) o al agotar los intentos.
/// El `watch` devuelto lleva las estadísticas de reconexión.
pub fn resumable<T, S, F, Fut>(
    after: Option<u64>,
    policy: ResumePolicy,
    mut open: F,
) -> (
    impl Stream<Item = Result<T, TransportError>>,
    tokio::sync::watch::Receiver<ResumeStats>,
)
where
    T: Cursor + Send + 'static,
    S: Stream<Item = Result<T, Status>> + Unpin + Send + 'static,
    F: FnMut(Option<u64>) -> Fut + Send + 'static,
    Fut: Future<Output = Result<S, Status>> + Send,
{
    let (tx, rx) = mpsc::channel(64);
    let (stats_tx, stats_rx) = tokio::sync::watch::channel(ResumeStats::default());
    tokio::spawn(async move {
        let mut last = after;
        let mut stats = ResumeStats::default();
        let mut failures_without_progress = 0u32;
        loop {
            let progress_before = last;
            let error = match open(last).await {
                Err(status) => TransportError::from(status),
                Ok(mut stream) => loop {
                    match stream.next().await {
                        None => {
                            let _ = stats_tx.send(stats);
                            return;
                        }
                        Some(Err(status)) => break TransportError::from(status),
                        Some(Ok(event)) => {
                            let cursor = event.cursor();
                            if last.is_some_and(|l| cursor <= l) {
                                stats.duplicates_dropped += 1;
                                continue;
                            }
                            last = Some(cursor);
                            if tx.send(Ok(event)).await.is_err() {
                                return; // el consumidor soltó el stream
                            }
                        }
                    }
                },
            };
            if last != progress_before {
                failures_without_progress = 0;
            }
            failures_without_progress += 1;
            if !error.is_retryable()
                || failures_without_progress > policy.max_attempts_without_progress
            {
                let _ = stats_tx.send(stats);
                let _ = tx.send(Err(error)).await;
                return;
            }
            stats.reconnects += 1;
            let _ = stats_tx.send(stats);
            tokio::time::sleep(policy.backoff).await;
        }
    });
    (ReceiverStream::new(rx), stats_rx)
}
