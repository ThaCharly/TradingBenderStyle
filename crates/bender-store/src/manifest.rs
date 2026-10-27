//! Manifiesto local: una línea JSON por segmento cerrado, en orden de cierre.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{StoreError, Zone};

/// Un segmento cerrado.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SegmentInfo {
    pub zone: Zone,
    /// Clave canónica de la fuente.
    pub source: String,
    /// Día UTC, "AAAA-MM-DD".
    pub date: String,
    /// Ruta relativa a la raíz del store, con `/`.
    pub path: String,
    pub rows: u64,
    pub ingest_seq_min: u64,
    pub ingest_seq_max: u64,
    /// Rango de `ts_available` (normalizada) o `ts_received` (cruda), ns UTC.
    pub ts_min: i64,
    pub ts_max: i64,
    /// BLAKE3 de la serialización canónica de las filas (igual en cualquier máquina).
    pub content_hash: String,
    /// BLAKE3 de los bytes del archivo Parquet (se verifica al leer).
    pub file_hash: String,
    /// Versión del normalizador (vacía en la zona cruda).
    pub normalizer_version: String,
}

pub struct Manifest {
    path: PathBuf,
    pub segments: Vec<SegmentInfo>,
}

impl Manifest {
    /// Carga el manifiesto. Una última línea incompleta (corte mientras se agregaba) se
    /// descarta y el archivo se reescribe sin ella.
    pub fn load(root: &Path) -> Result<Self, StoreError> {
        let path = root.join("manifest.jsonl");
        let mut segments = Vec::new();
        let mut dirty = false;
        if path.exists() {
            let lines: Vec<String> = BufReader::new(File::open(&path)?)
                .lines()
                .collect::<Result<_, _>>()?;
            let count = lines.len();
            for (i, line) in lines.into_iter().enumerate() {
                match serde_json::from_str(&line) {
                    Ok(info) => segments.push(info),
                    Err(_) if i + 1 == count => dirty = true,
                    Err(e) => {
                        return Err(StoreError::Decode(format!("manifest.jsonl:{}: {e}", i + 1)));
                    }
                }
            }
        }
        let manifest = Self { path, segments };
        if dirty {
            manifest.rewrite()?;
        }
        Ok(manifest)
    }

    fn rewrite(&self) -> Result<(), StoreError> {
        let tmp = self.path.with_extension("jsonl.tmp");
        let mut file = File::create(&tmp)?;
        for s in &self.segments {
            serde_json::to_writer(&mut file, s).map_err(|e| StoreError::Decode(e.to_string()))?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
        std::fs::rename(tmp, &self.path)?;
        Ok(())
    }

    /// Agrega un segmento y sincroniza el archivo.
    pub fn append(&mut self, info: SegmentInfo) -> Result<(), StoreError> {
        let mut line = serde_json::to_vec(&info).map_err(|e| StoreError::Decode(e.to_string()))?;
        line.push(b'\n');
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        file.write_all(&line)?;
        file.sync_all()?;
        self.segments.push(info);
        Ok(())
    }
}
