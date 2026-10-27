//! Segmentos Parquet append-only, inmutables y direccionables por contenido (doc 02 §2,
//! ADR-0007).
//!
//! - **Zonas**: cruda (el mensaje del exchange tal cual llegó, con `ts_received` e
//!   `ingest_seq`) y normalizada (el `Envelope` canónico y la versión del normalizador).
//! - **Partición**: `<raíz>/<zona>/<fuente>/<AAAA-MM-DD>/<ingest_seq inicial>-<hash>.parquet`.
//!   El día es el UTC de `ts_available` (normalizada) o de `ts_received` (cruda), y los
//!   archivos se comprimen con zstd.
//! - **Inmutabilidad**: un segmento cerrado tiene dos hashes BLAKE3: el *de contenido*, que se
//!   calcula sobre una serialización canónica de sus filas y es igual en cualquier máquina, y
//!   el *de archivo*, sobre los bytes del Parquet, que se verifica en cada lectura. Los dos
//!   se anotan en el manifiesto local `<raíz>/manifest.jsonl`.
//! - **Cortes**: el segmento abierto se escribe en un log (`open.wal`) con un checksum por
//!   registro. Al abrir el store, el prefijo válido de cada log se cierra como segmento y
//!   el resto se descarta, de forma determinista. Ver [`Store::open`].
//!
//! Un store lo usa un solo proceso a la vez.

mod civil;
mod error;
mod manifest;
mod segment;
mod store;
mod wal;

pub use error::StoreError;
pub use manifest::SegmentInfo;
pub use segment::RawRecord;
pub use store::{Store, Writer, WriterOptions};

/// Zona de almacenamiento (doc 02 §2).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
#[serde(rename_all = "lowercase")]
pub enum Zone {
    /// Mensajes del exchange tal cual llegaron. Es la verdad.
    Raw,
    /// `Envelope`s canónicos, re-derivables desde la cruda.
    Normalized,
}

impl Zone {
    pub fn dir_name(self) -> &'static str {
        match self {
            Self::Raw => "raw",
            Self::Normalized => "normalized",
        }
    }
}

/// Nombre de carpeta de una fuente: la clave canónica (`bender_types::source_key`) con
/// `:` → `~`, porque `:` no es válido en Windows, y cualquier carácter fuera de
/// `[A-Za-z0-9._-]` codificado como `%XX`.
pub fn source_dir(source_key: &str) -> String {
    let mut out = String::with_capacity(source_key.len());
    for b in source_key.bytes() {
        match b {
            b':' => out.push('~'),
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'.' | b'_' | b'-' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    #[test]
    fn source_dir_is_portable() {
        assert_eq!(
            super::source_dir("BINANCE:PERP_LINEAR:BTCUSDT:trades"),
            "BINANCE~PERP_LINEAR~BTCUSDT~trades"
        );
        assert_eq!(
            super::source_dir("USER:mis dibujos/1"),
            "USER~mis%20dibujos%2F1"
        );
    }
}
