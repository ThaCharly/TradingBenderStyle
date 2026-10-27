//! Estado versionado consultable `as_of` (doc 05 §7, doc 02 §3): cada cambio queda con el
//! `ts_available` en que se supo, y nada se sobrescribe.

use std::collections::BTreeMap;

/// Mapa clave → valor con historia. `None` en la historia = borrado.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VersionedStore<K: Ord, V> {
    history: BTreeMap<K, Vec<(i64, Option<V>)>>,
    last_ts: Option<i64>,
}

impl<K: Ord, V> Default for VersionedStore<K, V> {
    fn default() -> Self {
        Self {
            history: BTreeMap::new(),
            last_ts: None,
        }
    }
}

/// Un cambio llegó con `ts_available` anterior al último aplicado.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StaleUpdate {
    pub last_ts: i64,
    pub got_ts: i64,
}

impl std::fmt::Display for StaleUpdate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "cambio en {} después de uno en {}",
            self.got_ts, self.last_ts
        )
    }
}

impl std::error::Error for StaleUpdate {}

impl<K: Ord + Clone, V: Clone> VersionedStore<K, V> {
    pub fn new() -> Self {
        Self::default()
    }

    /// Registra que en `ts_available` la clave pasó a valer `value` (`None` = borrada). Los
    /// cambios se aplican en orden K, así que `ts_available` no puede retroceder; dos
    /// cambios con el mismo `ts_available` se aplican en orden y vale el último.
    pub fn apply(
        &mut self,
        ts_available: i64,
        key: K,
        value: Option<V>,
    ) -> Result<(), StaleUpdate> {
        if let Some(last) = self.last_ts
            && ts_available < last
        {
            return Err(StaleUpdate {
                last_ts: last,
                got_ts: ts_available,
            });
        }
        self.last_ts = Some(ts_available);
        self.history
            .entry(key)
            .or_default()
            .push((ts_available, value));
        Ok(())
    }

    /// Valor de `key` según lo sabido en `as_of` (cambios con `ts_available ≤ as_of`).
    pub fn get(&self, key: &K, as_of: i64) -> Option<&V> {
        let versions = self.history.get(key)?;
        let idx = versions.partition_point(|(ts, _)| *ts <= as_of);
        versions.get(idx.checked_sub(1)?)?.1.as_ref()
    }

    /// Todas las claves vivas en `as_of`, en orden de clave.
    pub fn list(&self, as_of: i64) -> impl Iterator<Item = (&K, &V)> {
        self.history
            .keys()
            .filter_map(move |k| self.get(k, as_of).map(|v| (k, v)))
    }

    /// Historia completa de una clave: `(ts_available, valor o borrado)`.
    pub fn history(&self, key: &K) -> &[(i64, Option<V>)] {
        self.history.get(key).map_or(&[], Vec::as_slice)
    }
}

// ---------------------------------------------------------------------------------------
// Serialización canónica (checkpoints).

use crate::canonical::{CheckpointError, Checkpointable, Decoder, Encoder};

impl<K: Ord + Checkpointable, V: Checkpointable> Checkpointable for VersionedStore<K, V> {
    fn encode(&self, e: &mut Encoder) {
        e.put(&self.history).put(&self.last_ts);
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        Ok(Self {
            history: d.get()?,
            last_ts: d.get()?,
        })
    }
}
