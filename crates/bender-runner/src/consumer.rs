//! Lo que ve un consumidor del Runner: el pasado ([`PastView`]) y, sólo para etiquetar en
//! modo `dataset`, una ventana del futuro ([`FutureWindow`]) que ningún consumidor puede
//! construir.

use bender_kernel::canonical::Checkpointable;
use bender_kernel::{BarAggregator, CoverageProjection, KernelOutput, KernelState, UserProjection};
use bender_types::proto;

/// Vista de sólo lectura del estado del núcleo **hasta el reloj actual**. No tiene
/// ninguna forma de leer eventos posteriores: el núcleo sólo conoce lo ya aplicado.
pub struct PastView<'a> {
    state: &'a KernelState,
    outputs: &'a [KernelOutput],
}

impl<'a> PastView<'a> {
    pub(crate) fn new(state: &'a KernelState, outputs: &'a [KernelOutput]) -> Self {
        Self { state, outputs }
    }

    /// `ts_available` del evento en curso.
    pub fn now(&self) -> i64 {
        self.state.now().unwrap_or(i64::MIN)
    }

    /// Velas de una fuente (en formación y cerradas que todavía se conservan).
    pub fn bars(&self, source: &str) -> Option<&'a BarAggregator> {
        self.state.bars(source)
    }

    /// Lo que emitió el núcleo al aplicar el evento en curso (velas cerradas o revisadas).
    pub fn kernel_outputs(&self) -> &'a [KernelOutput] {
        self.outputs
    }

    pub fn user(&self) -> &'a UserProjection {
        self.state.user()
    }

    pub fn coverage(&self) -> &'a CoverageProjection {
        self.state.coverage()
    }
}

/// Consumidor del Runner: motor, alertas, simulador de paper, features de un dataset…
/// Recibe cada evento en orden K, después de que el núcleo lo aplicó, y emite salidas
/// tipadas. Las salidas tienen forma canónica para poder compararlas por hash entre modos.
pub trait Consumer {
    type Output: Checkpointable;

    fn on_event(
        &mut self,
        event: &proto::Envelope,
        past: &PastView<'_>,
        out: &mut Vec<Self::Output>,
    );
}

/// Ventana de eventos **posteriores** a un instante, para calcular etiquetas en modo
/// `dataset`. Sólo la construye el Runner (no tiene constructor público), así que un
/// [`Consumer`] no tiene cómo obtener una: el sistema de tipos impide que una feature mire
/// el futuro.
pub struct FutureWindow<'a> {
    events: &'a [proto::Envelope],
    from: i64,
    horizon_end: i64,
}

impl<'a> FutureWindow<'a> {
    pub(crate) fn new(events: &'a [proto::Envelope], from: i64, horizon_end: i64) -> Self {
        Self {
            events,
            from,
            horizon_end,
        }
    }

    /// Inicio de la ventana: el instante del evento etiquetado (excluido).
    pub fn from(&self) -> i64 {
        self.from
    }

    /// Fin de la ventana (inclusive).
    pub fn horizon_end(&self) -> i64 {
        self.horizon_end
    }

    /// Eventos con `from < ts_available ≤ horizon_end`, en orden K.
    pub fn events(&self) -> &'a [proto::Envelope] {
        self.events
    }
}

/// Calcula etiquetas mirando hacia adelante. Sólo existe en modo `dataset`.
pub trait Labeler {
    /// Qué consumidor produce las filas que se etiquetan.
    type Row;

    /// Horizonte hacia adelante, en ns.
    fn horizon_ns(&self) -> i64;

    /// Etiqueta de `row`, emitida en `at` (ns), mirando la ventana posterior.
    fn label(&mut self, row: &Self::Row, at: i64, future: &FutureWindow<'_>) -> f64;
}
