//! Reloj del núcleo (doc 05 §3): el tiempo es el `ts_available` del evento que se está
//! procesando. Avanza en orden `K` y nunca retrocede; los temporizadores se disparan con el
//! primer evento (latido o no) cuyo `ts_available` alcanza su instante.

use std::collections::BTreeMap;

use crate::order::{Ordered, SourceOrder, compare};

/// Clave `K` del último evento procesado, sin asignar memoria por evento: la clave de la
/// fuente se guarda en un buffer reutilizado.
#[derive(Clone, Debug, Default)]
struct LastKey {
    ts_available: i64,
    source_seq: Option<u64>,
    ingest_seq: u64,
    source: SourceOrder,
}

impl Ordered for LastKey {
    fn ts_available(&self) -> i64 {
        self.ts_available
    }
    fn source_seq(&self) -> Option<u64> {
        self.source_seq
    }
    fn ingest_seq(&self) -> u64 {
        self.ingest_seq
    }
}

/// Un evento llegó con `K` menor o igual al último procesado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutOfOrder {
    pub current_ts: i64,
    pub current_source: String,
    pub current_ingest_seq: u64,
    pub got_ts: i64,
    pub got_source: String,
    pub got_ingest_seq: u64,
}

impl std::fmt::Display for OutOfOrder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "evento fuera de orden K: llegó ({}, {}, #{}) después de ({}, {}, #{})",
            self.got_ts,
            self.got_source,
            self.got_ingest_seq,
            self.current_ts,
            self.current_source,
            self.current_ingest_seq
        )
    }
}

impl std::error::Error for OutOfOrder {}

/// Identificador de un temporizador.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimerId(u64);

/// Un temporizador disparado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Fired<T> {
    pub id: TimerId,
    /// Instante pedido.
    pub at: i64,
    pub tag: T,
}

/// Reloj del núcleo con temporizadores. `T` es lo que el consumidor quiere recibir al
/// dispararse (p. ej. "cerrar la vela de 4h que empieza en t").
#[derive(Clone, Debug)]
pub struct KernelClock<T> {
    last: Option<LastKey>,
    timers: BTreeMap<(i64, TimerId), T>,
    next_id: u64,
}

impl<T> Default for KernelClock<T> {
    fn default() -> Self {
        Self {
            last: None,
            timers: BTreeMap::new(),
            next_id: 0,
        }
    }
}

impl<T> KernelClock<T> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tiempo actual: `ts_available` del último evento procesado.
    pub fn now(&self) -> Option<i64> {
        self.last.as_ref().map(|k| k.ts_available)
    }

    /// Pide un aviso cuando el reloj llegue a `at`. Si ya pasó, se dispara con el próximo
    /// evento. Los temporizadores con el mismo `at` se disparan en orden de creación.
    pub fn schedule(&mut self, at: i64, tag: T) -> TimerId {
        let id = TimerId(self.next_id);
        self.next_id += 1;
        self.timers.insert((at, id), tag);
        id
    }

    /// Cancela un temporizador pendiente. Devuelve su etiqueta si existía.
    pub fn cancel(&mut self, at: i64, id: TimerId) -> Option<T> {
        self.timers.remove(&(at, id))
    }

    pub fn pending(&self) -> usize {
        self.timers.len()
    }

    /// Avanza el reloj al evento `event` de la fuente `source`. Agrega a `fired`, en
    /// orden `(at, creación)`, los temporizadores con `at ≤ ts_available`: cada uno se
    /// dispara una sola vez. Los disparos corresponden a instantes anteriores o iguales al
    /// evento, así que el consumidor los procesa **antes** que el evento.
    ///
    /// Si el evento no es estrictamente posterior en `K` al último procesado, devuelve
    /// [`OutOfOrder`] y el reloj no cambia.
    pub fn advance<E: Ordered>(
        &mut self,
        event: &E,
        source: &SourceOrder,
        fired: &mut Vec<Fired<T>>,
    ) -> Result<(), OutOfOrder> {
        if let Some(last) = &self.last
            && compare(event, source, last, &last.source).is_le()
        {
            return Err(OutOfOrder {
                current_ts: last.ts_available,
                current_source: last.source.key.clone(),
                current_ingest_seq: last.ingest_seq,
                got_ts: event.ts_available(),
                got_source: source.key.clone(),
                got_ingest_seq: event.ingest_seq(),
            });
        }
        let last = self.last.get_or_insert_with(LastKey::default);
        last.ts_available = event.ts_available();
        last.source_seq = event.source_seq();
        last.ingest_seq = event.ingest_seq();
        last.source.rank = source.rank;
        last.source.key.clear();
        last.source.key.push_str(&source.key);

        let now = last.ts_available;
        while let Some(entry) = self.timers.first_entry() {
            if entry.key().0 > now {
                break;
            }
            let ((at, id), tag) = entry.remove_entry();
            fired.push(Fired { id, at, tag });
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------------------
// Serialización canónica (checkpoints).

use crate::canonical::{CheckpointError, Checkpointable, Decoder, Encoder};

impl Checkpointable for TimerId {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.0);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(Self(d.get()?))
    }
}

impl Checkpointable for SourceOrder {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.rank).put(&self.key);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(Self {
            rank: d.get()?,
            key: d.get()?,
        })
    }
}

crate::checkpoint_struct!(LastKey {
    ts_available,
    source_seq,
    ingest_seq,
    source
});

impl<T: Checkpointable> Checkpointable for KernelClock<T> {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.last).put(&self.timers).put(&self.next_id);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(Self {
            last: d.get()?,
            timers: d.get()?,
            next_id: d.get()?,
        })
    }
}

impl Checkpointable for () {
    fn encode(&self, _: &mut Encoder) {}
    fn decode(_: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(())
    }
}
