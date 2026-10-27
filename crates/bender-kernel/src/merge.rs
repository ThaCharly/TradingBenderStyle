//! Fusión k-way determinista de streams por fuente en un único stream en orden `K`.

use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

use crate::order::{Ordered, SourceOrder};

/// Error al armar una fusión.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MergeError {
    /// Dos streams con la misma fuente: el orden entre ellos no estaría definido.
    DuplicateSource(String),
}

impl std::fmt::Display for MergeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicateSource(k) => write!(f, "fuente repetida en la fusión: {k}"),
        }
    }
}

impl std::error::Error for MergeError {}

/// Entrada del heap: la clave `K` del evento en la cabeza de un stream. `ordinal` es la
/// posición de la fuente en el orden de [`SourceOrder`], así que comparar ordinales
/// equivale a comparar claves de fuente sin tocar strings.
#[derive(Clone, Copy, PartialEq, Eq)]
struct Head {
    ts_available: i64,
    rank: u32,
    source_seq: Option<u64>,
    ingest_seq: u64,
    ordinal: u32,
}

impl Ord for Head {
    fn cmp(&self, other: &Self) -> Ordering {
        self.ts_available
            .cmp(&other.ts_available)
            .then(self.rank.cmp(&other.rank))
            .then(self.source_seq.cmp(&other.source_seq))
            .then(self.ingest_seq.cmp(&other.ingest_seq))
            .then(self.ordinal.cmp(&other.ordinal))
    }
}

impl PartialOrd for Head {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

struct Stream<I: Iterator> {
    rank: u32,
    iter: I,
    head: Option<I::Item>,
}

/// Fusiona streams, cada uno ya ordenado por `K` dentro de su fuente (precondición: se
/// verifica con `debug_assert`), en un único
/// iterador en orden `K` (con la clave de la fuente como último desempate, ver
/// [`crate::order`]). El resultado no depende del orden en que se pasan los streams.
///
/// No asigna memoria por evento: el heap tiene un lugar por stream y los eventos no se
/// copian a él.
pub struct Merge<I: Iterator> {
    streams: Vec<Stream<I>>,
    heap: BinaryHeap<Reverse<Head>>,
}

impl<T: Ordered, I: Iterator<Item = T>> Merge<I> {
    pub fn new(streams: impl IntoIterator<Item = (SourceOrder, I)>) -> Result<Self, MergeError> {
        let mut streams: Vec<(SourceOrder, I)> = streams.into_iter().collect();
        streams.sort_by(|a, b| a.0.cmp(&b.0));
        if let Some(w) = streams.windows(2).find(|w| w[0].0 == w[1].0) {
            return Err(MergeError::DuplicateSource(w[0].0.key.clone()));
        }
        let mut merge = Self {
            heap: BinaryHeap::with_capacity(streams.len()),
            streams: streams
                .into_iter()
                .map(|(order, iter)| Stream {
                    rank: order.rank,
                    iter,
                    head: None,
                })
                .collect(),
        };
        for ordinal in 0..merge.streams.len() {
            merge.advance(ordinal);
        }
        Ok(merge)
    }

    /// Carga el próximo evento del stream `ordinal` y lo anota en el heap.
    fn advance(&mut self, ordinal: usize) -> Option<Head> {
        let stream = &mut self.streams[ordinal];
        let item = stream.iter.next()?;
        {
            let head = Head {
                ts_available: item.ts_available(),
                rank: stream.rank,
                source_seq: item.source_seq(),
                ingest_seq: item.ingest_seq(),
                ordinal: ordinal as u32,
            };
            stream.head = Some(item);
            self.heap.push(Reverse(head));
            Some(head)
        }
    }
}

impl<T: Ordered, I: Iterator<Item = T>> Iterator for Merge<I> {
    type Item = T;

    fn next(&mut self) -> Option<T> {
        let Reverse(head) = self.heap.pop()?;
        let ordinal = head.ordinal as usize;
        let item = self.streams[ordinal].head.take().expect("cabeza cargada");
        let next = self.advance(ordinal);
        debug_assert!(
            next.is_none_or(|n| n > head),
            "stream de entrada fuera de orden K dentro de su fuente"
        );
        Some(item)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let (lo, hi) = self
            .streams
            .iter()
            .map(|s| s.iter.size_hint())
            .fold((0usize, Some(0usize)), |(lo, hi), (l, h)| {
                (lo + l, hi.zip(h).map(|(a, b)| a + b))
            });
        let heads = self.heap.len();
        (lo + heads, hi.map(|h| h + heads))
    }
}
