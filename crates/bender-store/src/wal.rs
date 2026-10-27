//! Log del segmento abierto (`open.wal`).
//!
//! Formato: una línea de cabecera `BWAL1 <json>\n` y después registros
//! `[len u32][ingest_seq u64][ts i64][datos; len bytes][checksum u32]`, todo little-endian,
//! con checksum = primeros 4 bytes de BLAKE3(ingest_seq ‖ ts ‖ datos). Al recuperar se
//! toma el prefijo de registros completos y válidos; lo que sigue se descarta.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Read, Write};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::StoreError;
use crate::Zone;
use crate::segment::Row;

const MAGIC: &[u8] = b"BWAL1 ";

/// Datos fijos del segmento abierto.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct WalHeader {
    pub zone: Zone,
    pub source: String,
    pub normalizer_version: String,
    pub day: i64,
}

pub struct Wal {
    file: BufWriter<File>,
}

fn checksum(ingest_seq: u64, ts: i64, data: &[u8]) -> [u8; 4] {
    let mut h = blake3::Hasher::new();
    h.update(&ingest_seq.to_le_bytes());
    h.update(&ts.to_le_bytes());
    h.update(data);
    let out = h.finalize();
    [
        out.as_bytes()[0],
        out.as_bytes()[1],
        out.as_bytes()[2],
        out.as_bytes()[3],
    ]
}

impl Wal {
    pub fn create(path: &Path, header: &WalHeader) -> Result<Self, StoreError> {
        let mut file = BufWriter::new(
            OpenOptions::new()
                .create(true)
                .truncate(true)
                .write(true)
                .open(path)?,
        );
        file.write_all(MAGIC)?;
        serde_json::to_writer(&mut file, header).map_err(|e| StoreError::Decode(e.to_string()))?;
        file.write_all(b"\n")?;
        file.flush()?;
        Ok(Self { file })
    }

    /// Agrega un registro y lo entrega al sistema operativo: si el proceso muere, el
    /// registro no se pierde (ante un corte de energía, sólo lo ya sincronizado).
    pub fn append(&mut self, row: &Row) -> Result<(), StoreError> {
        let len = u32::try_from(row.data.len())
            .map_err(|_| StoreError::Decode("registro de más de 4 GiB".into()))?;
        self.file.write_all(&len.to_le_bytes())?;
        self.file.write_all(&row.ingest_seq.to_le_bytes())?;
        self.file.write_all(&row.ts.to_le_bytes())?;
        self.file.write_all(&row.data)?;
        self.file
            .write_all(&checksum(row.ingest_seq, row.ts, &row.data))?;
        self.file.flush()?;
        Ok(())
    }
}

/// Lee un log: cabecera y prefijo válido de registros. `None` si la cabecera está
/// incompleta (el log se creó pero no llegó a escribirse).
pub fn recover(path: &Path) -> Result<Option<(WalHeader, Vec<Row>)>, StoreError> {
    let mut bytes = Vec::new();
    File::open(path)?.read_to_end(&mut bytes)?;
    let Some(rest) = bytes.strip_prefix(MAGIC) else {
        return Ok(None);
    };
    let Some(newline) = rest.iter().position(|&b| b == b'\n') else {
        return Ok(None);
    };
    let Ok(header) = serde_json::from_slice::<WalHeader>(&rest[..newline]) else {
        return Ok(None);
    };
    let mut rows = Vec::new();
    let mut cursor = &rest[newline + 1..];
    loop {
        if cursor.len() < 20 {
            break;
        }
        let len = u32::from_le_bytes(cursor[0..4].try_into().expect("4 bytes")) as usize;
        let Some(total) = len.checked_add(24) else {
            break;
        };
        if cursor.len() < total {
            break;
        }
        let ingest_seq = u64::from_le_bytes(cursor[4..12].try_into().expect("8 bytes"));
        let ts = i64::from_le_bytes(cursor[12..20].try_into().expect("8 bytes"));
        let data = &cursor[20..20 + len];
        if cursor[20 + len..total] != checksum(ingest_seq, ts, data) {
            break;
        }
        rows.push(Row {
            ingest_seq,
            ts,
            data: data.to_vec(),
        });
        cursor = &cursor[total..];
    }
    Ok(Some((header, rows)))
}
