//! Un solo Runner con cuatro modos (doc 05 §9, ADR-0004): `live`, `replay`, `backtest` y
//! `dataset` son el mismo código con distinta fuente de eventos.
//!
//! - El Runner aplica cada evento al núcleo ([`KernelState`]) y después se lo pasa al
//!   [`Consumer`] con una [`PastView`] (sólo pasado).
//! - **backtest**: lo más rápido posible ([`backtest`]).
//! - **replay**: el mismo bucle, controlado paso a paso ([`Replay`]: avanzar N, ir a T,
//!   reproducir con un [`Pacer`] y pausar).
//! - **live**: el mismo bucle sobre una [`LiveSource`] ([`live`]).
//! - **dataset**: features de un consumidor más etiquetas de un [`Labeler`] con una
//!   [`FutureWindow`] que sólo el Runner puede construir ([`dataset`]).
//! - Cada corrida produce su [`RunManifest`].
//!
//! Este crate no lee el reloj del sistema (Pilar 2): el ritmo del replay lo impone un
//! [`Pacer`] que provee quien lo usa.

mod consumer;
mod dataset;
pub mod golden;
pub mod normative;
mod source;

use std::collections::BTreeMap;

use bender_kernel::canonical::{self, Checkpointable};
use bender_kernel::{KernelConfig, KernelError, KernelOutput, KernelState, RunManifest};
use bender_types::proto;

pub use consumer::{Consumer, FutureWindow, Labeler, PastView};
pub use dataset::{DatasetRow, dataset, write_dataset_parquet};
pub use source::{EventSource, LiveSource, SourceError, StoreSource, VecSource};

/// Commit con el que se compiló (variable `BENDER_GIT_COMMIT` al compilar).
pub const GIT_COMMIT: &str = match option_env!("BENDER_GIT_COMMIT") {
    Some(c) => c,
    None => "desconocido",
};

/// Error de una corrida.
#[derive(Debug)]
pub enum RunError {
    Kernel(KernelError),
    Source(SourceError),
}

impl std::fmt::Display for RunError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Kernel(e) => write!(f, "{e}"),
            Self::Source(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for RunError {}

/// Núcleo + consumidor. Es el corazón de todos los modos.
pub struct Runner<C: Consumer> {
    state: KernelState,
    consumer: C,
    kernel_out: Vec<KernelOutput>,
    consumer_hash: [u8; 32],
    outputs: u64,
}

impl<C: Consumer> Runner<C> {
    pub fn new(config: KernelConfig, consumer: C) -> Self {
        Self {
            state: KernelState::new(config),
            consumer,
            kernel_out: Vec::new(),
            consumer_hash: [0; 32],
            outputs: 0,
        }
    }

    /// Runner que retoma desde un estado del núcleo (p. ej. un checkpoint).
    pub fn from_state(state: KernelState, consumer: C) -> Self {
        Self {
            state,
            consumer,
            kernel_out: Vec::new(),
            consumer_hash: [0; 32],
            outputs: 0,
        }
    }

    pub fn state(&self) -> &KernelState {
        &self.state
    }

    pub fn consumer(&self) -> &C {
        &self.consumer
    }

    /// Cantidad de salidas emitidas por el consumidor.
    pub fn outputs(&self) -> u64 {
        self.outputs
    }

    /// Procesa un evento y agrega las salidas del consumidor a `out`.
    pub fn process(
        &mut self,
        event: &proto::Envelope,
        out: &mut Vec<C::Output>,
    ) -> Result<(), RunError> {
        self.kernel_out.clear();
        self.state
            .apply(event, &mut self.kernel_out)
            .map_err(RunError::Kernel)?;
        let first = out.len();
        let past = PastView::new(&self.state, &self.kernel_out);
        self.consumer.on_event(event, &past, out);
        for o in &out[first..] {
            let mut h = blake3::Hasher::new();
            h.update(&self.consumer_hash);
            h.update(&canonical::to_bytes(o));
            self.consumer_hash = *h.finalize().as_bytes();
            self.outputs += 1;
        }
        Ok(())
    }

    /// Hash de salida de la corrida: núcleo y consumidor.
    pub fn output_hash(&self) -> String {
        let mut h = blake3::Hasher::new();
        h.update(self.state.output_hash().as_bytes());
        h.update(&self.consumer_hash);
        h.finalize().to_hex().to_string()
    }

    /// Manifiesto de la corrida hasta acá.
    pub fn manifest(&self, segments: BTreeMap<String, String>) -> RunManifest {
        RunManifest {
            segments,
            git_commit: GIT_COMMIT.to_string(),
            config_hash: self.state.config().hash(),
            output_hash: self.output_hash(),
        }
    }
}

/// Resultado de una corrida completa.
pub struct RunResult<C: Consumer> {
    pub runner: Runner<C>,
    pub outputs: Vec<C::Output>,
    pub manifest: RunManifest,
}

fn run_all<C: Consumer>(
    config: KernelConfig,
    consumer: C,
    source: &mut dyn EventSource,
) -> Result<RunResult<C>, RunError> {
    let mut runner = Runner::new(config, consumer);
    let mut outputs = Vec::new();
    while let Some(event) = source.next_event() {
        let event = event.map_err(RunError::Source)?;
        runner.process(&event, &mut outputs)?;
    }
    let manifest = runner.manifest(source.segments());
    Ok(RunResult {
        runner,
        outputs,
        manifest,
    })
}

/// Modo `backtest`: todo lo más rápido posible.
pub fn backtest<C: Consumer>(
    config: KernelConfig,
    consumer: C,
    source: &mut dyn EventSource,
) -> Result<RunResult<C>, RunError> {
    run_all(config, consumer, source)
}

/// Modo `live`: el mismo bucle sobre una fuente en vivo, hasta que se cierra.
pub fn live<C: Consumer>(
    config: KernelConfig,
    consumer: C,
    source: &mut LiveSource,
) -> Result<RunResult<C>, RunError> {
    run_all(config, consumer, source)
}

/// Qué hacer después de esperar al próximo evento.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pace {
    Continue,
    Pause,
}

/// Ritmo del replay. Lo implementa quien usa el Runner (p. ej. con el reloj de pared a
/// ×1 o ×10); el Runner no lee el reloj.
pub trait Pacer {
    /// Llamado antes de procesar un evento con `ts_available = ts`.
    fn wait_until(&mut self, ts: i64) -> Pace;
}

/// Modo `replay` sobre un log grabado, controlado por API: avanzar N eventos, ir a un
/// instante (hacia adelante o hacia atrás), reproducir con un [`Pacer`] y pausar.
pub struct Replay<C: Consumer, F: Fn() -> C> {
    config: KernelConfig,
    events: Vec<proto::Envelope>,
    make_consumer: F,
    position: usize,
    runner: Runner<C>,
    outputs: Vec<C::Output>,
}

impl<C: Consumer, F: Fn() -> C> Replay<C, F> {
    pub fn new(config: KernelConfig, events: Vec<proto::Envelope>, make_consumer: F) -> Self {
        let runner = Runner::new(config.clone(), make_consumer());
        Self {
            config,
            events,
            make_consumer,
            position: 0,
            runner,
            outputs: Vec::new(),
        }
    }

    /// Eventos ya procesados.
    pub fn position(&self) -> usize {
        self.position
    }

    pub fn is_finished(&self) -> bool {
        self.position == self.events.len()
    }

    /// Instante actual (`ts_available` del último evento procesado).
    pub fn now(&self) -> Option<i64> {
        self.runner.state.now()
    }

    pub fn runner(&self) -> &Runner<C> {
        &self.runner
    }

    /// Salidas emitidas hasta ahora.
    pub fn outputs(&self) -> &[C::Output] {
        &self.outputs
    }

    /// Avanza hasta `n` eventos. Devuelve cuántos procesó.
    pub fn step(&mut self, n: usize) -> Result<usize, RunError> {
        let end = (self.position + n).min(self.events.len());
        let start = self.position;
        while self.position < end {
            self.runner
                .process(&self.events[self.position], &mut self.outputs)?;
            self.position += 1;
        }
        Ok(end - start)
    }

    /// Lleva el replay al estado en `t`: procesados todos los eventos con
    /// `ts_available ≤ t`. Hacia atrás, reconstruye desde el inicio (los consumidores no
    /// guardan checkpoints).
    pub fn seek(&mut self, t: i64) -> Result<(), RunError> {
        let target = self.events.partition_point(|e| e.ts_available <= t);
        if target < self.position {
            self.runner = Runner::new(self.config.clone(), (self.make_consumer)());
            self.outputs.clear();
            self.position = 0;
        }
        self.step(target - self.position).map(|_| ())
    }

    /// Reproduce hasta el final o hasta que el `pacer` pida pausa. Devuelve los eventos
    /// procesados.
    pub fn play(&mut self, pacer: &mut dyn Pacer) -> Result<usize, RunError> {
        let start = self.position;
        while let Some(event) = self.events.get(self.position) {
            if pacer.wait_until(event.ts_available) == Pace::Pause {
                break;
            }
            self.step(1)?;
        }
        Ok(self.position - start)
    }

    /// Manifiesto de lo reproducido hasta acá.
    pub fn manifest(&self) -> RunManifest {
        self.runner.manifest(BTreeMap::new())
    }
}

/// Bytes canónicos de un valor (para comparar salidas entre modos).
pub fn canonical_bytes<T: Checkpointable>(value: &T) -> Vec<u8> {
    canonical::to_bytes(value)
}
