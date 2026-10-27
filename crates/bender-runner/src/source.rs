//! Fuentes de eventos del Runner. Todas entregan `Envelope`s en orden K.

use std::collections::BTreeMap;
use std::sync::mpsc;

use bender_kernel::{Merge, SourceRankTable};
use bender_store::{Store, StoreError, WriterOptions, Zone};
use bender_types::proto;

/// Error de una fuente.
#[derive(Debug)]
pub enum SourceError {
    Store(StoreError),
    UnknownSource(String),
}

impl std::fmt::Display for SourceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Store(e) => write!(f, "{e}"),
            Self::UnknownSource(s) => write!(f, "fuente sin source_rank: {s}"),
        }
    }
}

impl std::error::Error for SourceError {}

impl From<StoreError> for SourceError {
    fn from(e: StoreError) -> Self {
        Self::Store(e)
    }
}

/// Fuente de eventos en orden K.
pub trait EventSource {
    /// Próximo evento; `None` = terminó.
    fn next_event(&mut self) -> Option<Result<proto::Envelope, SourceError>>;

    /// Segmentos leídos (ruta → hash de contenido), para el manifiesto.
    fn segments(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}

/// Eventos ya en memoria (en orden K).
pub struct VecSource(std::vec::IntoIter<proto::Envelope>);

impl VecSource {
    pub fn new(events: Vec<proto::Envelope>) -> Self {
        Self(events.into_iter())
    }
}

impl EventSource for VecSource {
    fn next_event(&mut self) -> Option<Result<proto::Envelope, SourceError>> {
        self.0.next().map(Ok)
    }
}

/// Log grabado o reconstruido en `bender-store`: lee la zona normalizada de varias
/// fuentes en `[desde, hasta)` de `ts_available` y las fusiona en orden K.
pub struct StoreSource {
    events: std::vec::IntoIter<proto::Envelope>,
    segments: BTreeMap<String, String>,
}

impl StoreSource {
    pub fn open(
        store: &Store,
        sources: &[String],
        from: i64,
        to: i64,
    ) -> Result<Self, SourceError> {
        let table = SourceRankTable::current();
        let mut streams = Vec::new();
        let mut segments = BTreeMap::new();
        for source in sources {
            let events = store.read_normalized(source, from, to)?;
            for s in store.segments().iter().filter(|s| {
                s.zone == Zone::Normalized
                    && &s.source == source
                    && s.ts_max >= from
                    && s.ts_min < to
            }) {
                segments.insert(s.path.clone(), s.content_hash.clone());
            }
            let Some(first) = events.first() else {
                continue;
            };
            let order = first
                .source
                .as_ref()
                .and_then(|s| table.source_order(s).ok())
                .ok_or_else(|| SourceError::UnknownSource(source.clone()))?;
            streams.push((order, events.into_iter()));
        }
        let merged: Vec<proto::Envelope> = Merge::new(streams)
            .map_err(|e| SourceError::UnknownSource(e.to_string()))?
            .collect();
        Ok(Self {
            events: merged.into_iter(),
            segments,
        })
    }
}

impl EventSource for StoreSource {
    fn next_event(&mut self) -> Option<Result<proto::Envelope, SourceError>> {
        self.events.next().map(Ok)
    }

    fn segments(&self) -> BTreeMap<String, String> {
        self.segments.clone()
    }
}

/// Fuente en vivo: recibe los eventos ya fusionados en orden K (de los conectores y el
/// generador de latidos) por un canal y, si tiene store, los graba en la zona normalizada
/// a medida que pasan.
pub struct LiveSource {
    rx: mpsc::Receiver<proto::Envelope>,
    store: Option<(Store, String)>,
    options: WriterOptions,
}

impl LiveSource {
    pub fn new(rx: mpsc::Receiver<proto::Envelope>) -> Self {
        Self {
            rx,
            store: None,
            options: WriterOptions::default(),
        }
    }

    /// Graba cada evento en `store` con esta versión del normalizador.
    pub fn recording(mut self, store: Store, normalizer_version: &str) -> Self {
        self.store = Some((store, normalizer_version.to_string()));
        self
    }

    /// Cierra los segmentos abiertos y devuelve el store.
    pub fn finish(self) -> Result<Option<Store>, SourceError> {
        match self.store {
            Some((mut store, _)) => {
                store.close_all()?;
                Ok(Some(store))
            }
            None => Ok(None),
        }
    }
}

impl EventSource for LiveSource {
    fn next_event(&mut self) -> Option<Result<proto::Envelope, SourceError>> {
        let event = self.rx.recv().ok()?;
        if let Some((store, version)) = &mut self.store {
            let key = event
                .source
                .as_ref()
                .map(bender_types::source_key)
                .unwrap_or_default();
            if let Err(e) = store.append_envelope(&key, version, &self.options, &event) {
                return Some(Err(e.into()));
            }
        }
        Some(Ok(event))
    }
}
