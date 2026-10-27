//! El store: escritores por fuente, cierre de segmentos, recuperación y lectura.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};

use bender_types::proto;
use prost::Message;

use crate::civil::{date_string, day_number};
use crate::manifest::{Manifest, SegmentInfo};
use crate::segment::{RawRecord, Row, content_hash, read_parquet, write_parquet};
use crate::wal::{self, Wal, WalHeader};
use crate::{StoreError, Zone, source_dir};

const WAL_NAME: &str = "open.wal";

/// Opciones de un escritor.
#[derive(Clone, Debug)]
pub struct WriterOptions {
    /// Filas máximas por segmento: al llegar, se cierra y se abre otro.
    pub max_rows: usize,
}

impl Default for WriterOptions {
    fn default() -> Self {
        Self { max_rows: 500_000 }
    }
}

/// Store de segmentos en una carpeta.
pub struct Store {
    root: PathBuf,
    manifest: Manifest,
}

/// Punto de un cierre en el que se simula un corte (sólo tests).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) enum CrashPoint {
    None,
    /// Parquet temporal escrito, sin renombrar.
    AfterTmp,
    /// Parquet renombrado, sin anotar en el manifiesto.
    AfterRename,
    /// Anotado en el manifiesto, con el log todavía presente.
    AfterManifest,
}

impl Store {
    /// Abre (o crea) un store y deja todo consistente después de un corte:
    ///
    /// 1. descarta una última línea incompleta del manifiesto;
    /// 2. borra los Parquet temporales (`*.parquet.tmp`) y los Parquet que no están en el
    ///    manifiesto (se cortó entre el renombre y la anotación);
    /// 3. por cada `open.wal`, toma el prefijo de registros válidos y lo cierra como
    ///    segmento, salvo que ese mismo contenido ya esté en el manifiesto (se cortó
    ///    después de anotarlo). El log se borra.
    ///
    /// Es determinista: el mismo estado en disco da el mismo resultado.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self, StoreError> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        let manifest = Manifest::load(&root)?;
        let mut store = Self { root, manifest };
        store.recover()?;
        Ok(store)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Segmentos cerrados, en orden de cierre.
    pub fn segments(&self) -> &[SegmentInfo] {
        &self.manifest.segments
    }

    fn recover(&mut self) -> Result<(), StoreError> {
        let known: std::collections::BTreeSet<String> = self
            .manifest
            .segments
            .iter()
            .map(|s| s.path.clone())
            .collect();
        let mut wals = Vec::new();
        for zone in [Zone::Raw, Zone::Normalized] {
            let dir = self.root.join(zone.dir_name());
            if !dir.exists() {
                continue;
            }
            let mut files = Vec::new();
            walk(&dir, &mut files)?;
            files.sort();
            for path in files {
                let name = path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default();
                let orphan = name.ends_with(".parquet") && !known.contains(&self.relative(&path));
                if name.ends_with(".parquet.tmp") || orphan {
                    std::fs::remove_file(&path)?;
                } else if name == WAL_NAME {
                    wals.push(path);
                }
            }
        }
        for path in wals {
            match wal::recover(&path)? {
                Some((header, rows)) if !rows.is_empty() => {
                    let hash = content_hash(
                        header.zone,
                        &header.source,
                        &header.normalizer_version,
                        &rows,
                    );
                    let already = self.manifest.segments.iter().any(|s| {
                        s.zone == header.zone && s.source == header.source && s.content_hash == hash
                    });
                    if !already {
                        self.close_segment(&header, &rows, CrashPoint::None)?;
                    }
                }
                _ => {}
            }
            std::fs::remove_file(&path)?;
        }
        Ok(())
    }

    fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect::<Vec<_>>()
            .join("/")
    }

    fn source_path(&self, zone: Zone, source: &str) -> PathBuf {
        self.root.join(zone.dir_name()).join(source_dir(source))
    }

    /// Último `ingest_seq` cerrado de una fuente (para retomar después de reiniciar).
    pub fn last_ingest_seq(&self, zone: Zone, source: &str) -> Option<u64> {
        self.manifest
            .segments
            .iter()
            .filter(|s| s.zone == zone && s.source == source)
            .map(|s| s.ingest_seq_max)
            .max()
    }

    /// Escribe un segmento cerrado: Parquet temporal → sincronizar → renombrar →
    /// manifiesto. El log del segmento abierto lo borra quien llama.
    fn close_segment(
        &mut self,
        header: &WalHeader,
        rows: &[Row],
        crash: CrashPoint,
    ) -> Result<Option<SegmentInfo>, StoreError> {
        let hash = content_hash(
            header.zone,
            &header.source,
            &header.normalizer_version,
            rows,
        );
        let date = date_string(header.day);
        let dir = self.source_path(header.zone, &header.source).join(&date);
        std::fs::create_dir_all(&dir)?;
        let first = rows.first().map_or(0, |r| r.ingest_seq);
        let final_path = dir.join(format!("{first:020}-{}.parquet", &hash[..16]));
        let tmp_path = dir.join(format!("{first:020}-{}.parquet.tmp", &hash[..16]));

        let bytes = write_parquet(
            header.zone,
            &header.source,
            &header.normalizer_version,
            rows,
        )?;
        let file_hash = blake3::hash(&bytes).to_hex().to_string();
        let mut file = File::create(&tmp_path)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        if crash == CrashPoint::AfterTmp {
            return Ok(None);
        }
        std::fs::rename(&tmp_path, &final_path)?;
        sync_dir(&dir);
        if crash == CrashPoint::AfterRename {
            return Ok(None);
        }
        let info = SegmentInfo {
            zone: header.zone,
            source: header.source.clone(),
            date,
            path: self.relative(&final_path),
            rows: rows.len() as u64,
            ingest_seq_min: first,
            ingest_seq_max: rows.last().map_or(0, |r| r.ingest_seq),
            ts_min: rows.iter().map(|r| r.ts).min().unwrap_or_default(),
            ts_max: rows.iter().map(|r| r.ts).max().unwrap_or_default(),
            content_hash: hash,
            file_hash,
            normalizer_version: header.normalizer_version.clone(),
        };
        self.manifest.append(info.clone())?;
        Ok(Some(info))
    }

    /// Escritor de la zona normalizada para una fuente.
    pub fn normalized_writer(
        &mut self,
        source: &str,
        normalizer_version: &str,
        options: WriterOptions,
    ) -> Writer<'_> {
        self.writer(Zone::Normalized, source, normalizer_version, options)
    }

    /// Escritor de la zona cruda para una fuente.
    pub fn raw_writer(&mut self, source: &str, options: WriterOptions) -> Writer<'_> {
        self.writer(Zone::Raw, source, "", options)
    }

    fn writer(
        &mut self,
        zone: Zone,
        source: &str,
        normalizer_version: &str,
        options: WriterOptions,
    ) -> Writer<'_> {
        let last = self.last_ingest_seq(zone, source);
        Writer {
            store: self,
            zone,
            source: source.to_string(),
            normalizer_version: normalizer_version.to_string(),
            options,
            last,
            open: None,
        }
    }

    fn read_rows(
        &self,
        zone: Zone,
        source: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<Row>, StoreError> {
        let mut segments: Vec<&SegmentInfo> = self
            .manifest
            .segments
            .iter()
            .filter(|s| s.zone == zone && s.source == source && s.ts_max >= from && s.ts_min < to)
            .collect();
        segments.sort_by_key(|s| s.ingest_seq_min);
        let mut out = Vec::new();
        for s in segments {
            let path = self.root.join(&s.path);
            let bytes = std::fs::read(&path).map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => StoreError::MissingSegment(path.clone()),
                _ => StoreError::Io(e),
            })?;
            let actual = blake3::hash(&bytes).to_hex().to_string();
            if actual != s.file_hash {
                return Err(StoreError::Integrity {
                    path,
                    expected: s.file_hash.clone(),
                    actual,
                });
            }
            let rows = read_parquet(zone, bytes)?;
            let content = content_hash(zone, &s.source, &s.normalizer_version, &rows);
            if content != s.content_hash {
                return Err(StoreError::Integrity {
                    path,
                    expected: s.content_hash.clone(),
                    actual: content,
                });
            }
            out.extend(rows.into_iter().filter(|r| r.ts >= from && r.ts < to));
        }
        Ok(out)
    }

    /// `Envelope`s de una fuente con `ts_available` en `[from, to)`, en orden de
    /// `ingest_seq`. Verifica la integridad de cada segmento leído.
    pub fn read_normalized(
        &self,
        source: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<proto::Envelope>, StoreError> {
        self.read_rows(Zone::Normalized, source, from, to)?
            .into_iter()
            .map(|r| {
                proto::Envelope::decode(r.data.as_slice())
                    .map_err(|e| StoreError::Decode(e.to_string()))
            })
            .collect()
    }

    /// Registros crudos de una fuente con `ts_received` en `[from, to)`, en orden de
    /// `ingest_seq`.
    pub fn read_raw(&self, source: &str, from: i64, to: i64) -> Result<Vec<RawRecord>, StoreError> {
        Ok(self
            .read_rows(Zone::Raw, source, from, to)?
            .into_iter()
            .map(|r| RawRecord {
                ts_received: r.ts,
                ingest_seq: r.ingest_seq,
                payload: r.data,
            })
            .collect())
    }

    /// Cantidad de segmentos por fuente y zona (para reportes).
    pub fn summary(&self) -> BTreeMap<(Zone, String), (usize, u64)> {
        let mut out = BTreeMap::new();
        for s in &self.manifest.segments {
            let entry = out.entry((s.zone, s.source.clone())).or_insert((0, 0));
            entry.0 += 1;
            entry.1 += s.rows;
        }
        out
    }
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), StoreError> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            walk(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// Sincroniza la carpeta para que el renombre sobreviva a un corte de energía (en los
/// sistemas que lo admiten; en Windows no hace nada).
fn sync_dir(dir: &Path) {
    if let Ok(d) = File::open(dir) {
        let _ = d.sync_all();
    }
}

struct OpenSegment {
    header: WalHeader,
    wal: Wal,
    wal_path: PathBuf,
    rows: Vec<Row>,
}

/// Escritor append-only de una fuente. Cada `append` queda en el log del segmento
/// abierto; [`Writer::close`] (o el cambio de día, o `max_rows`) lo cierra como segmento.
/// Si el escritor se descarta sin cerrar, el segmento abierto se recupera la próxima vez
/// que se abra el store.
pub struct Writer<'a> {
    store: &'a mut Store,
    zone: Zone,
    source: String,
    normalizer_version: String,
    options: WriterOptions,
    last: Option<u64>,
    open: Option<OpenSegment>,
}

impl Writer<'_> {
    /// Agrega un `Envelope` (zona normalizada). Su `ingest_seq` tiene que ser mayor que el
    /// último escrito de la fuente.
    pub fn append_envelope(&mut self, envelope: &proto::Envelope) -> Result<(), StoreError> {
        debug_assert_eq!(self.zone, Zone::Normalized);
        self.append_row(Row {
            ingest_seq: envelope.ingest_seq,
            ts: envelope.ts_available,
            data: envelope.encode_to_vec(),
        })
    }

    /// Agrega un mensaje crudo.
    pub fn append_raw(&mut self, record: &RawRecord) -> Result<(), StoreError> {
        debug_assert_eq!(self.zone, Zone::Raw);
        self.append_row(Row {
            ingest_seq: record.ingest_seq,
            ts: record.ts_received,
            data: record.payload.clone(),
        })
    }

    fn append_row(&mut self, row: Row) -> Result<(), StoreError> {
        if let Some(last) = self.last
            && row.ingest_seq <= last
        {
            return Err(StoreError::OutOfOrder {
                source: self.source.clone(),
                last,
                got: row.ingest_seq,
            });
        }
        let day = day_number(row.ts);
        if self.open.as_ref().is_some_and(|o| o.header.day != day) {
            self.close()?;
        }
        if self.open.is_none() {
            let dir = self.store.source_path(self.zone, &self.source);
            std::fs::create_dir_all(&dir)?;
            let header = WalHeader {
                zone: self.zone,
                source: self.source.clone(),
                normalizer_version: self.normalizer_version.clone(),
                day,
            };
            let wal_path = dir.join(WAL_NAME);
            let wal = Wal::create(&wal_path, &header)?;
            self.open = Some(OpenSegment {
                header,
                wal,
                wal_path,
                rows: Vec::new(),
            });
        }
        let open = self.open.as_mut().expect("segmento abierto");
        open.wal.append(&row)?;
        self.last = Some(row.ingest_seq);
        open.rows.push(row);
        if open.rows.len() >= self.options.max_rows {
            self.close()?;
        }
        Ok(())
    }

    /// Cierra el segmento abierto, si hay. Devuelve su información.
    pub fn close(&mut self) -> Result<Option<SegmentInfo>, StoreError> {
        self.close_with(CrashPoint::None)
    }

    pub(crate) fn close_with(
        &mut self,
        crash: CrashPoint,
    ) -> Result<Option<SegmentInfo>, StoreError> {
        let Some(open) = self.open.take() else {
            return Ok(None);
        };
        drop(open.wal);
        let info = self.store.close_segment(&open.header, &open.rows, crash)?;
        if crash == CrashPoint::None {
            std::fs::remove_file(&open.wal_path)?;
        }
        Ok(info)
    }
}

#[cfg(test)]
mod crash_tests {
    //! Criterio de #22: un corte a mitad de escritura no deja duplicados ni segmentos
    //! corruptos visibles después de reiniciar.

    use super::*;
    use bender_types::proto::{Envelope, Heartbeat, envelope};

    fn env(seq: u64) -> Envelope {
        Envelope {
            ts_event: 1_761_600_000_000_000_000 + seq as i64 * 1_000_000,
            ts_received: None,
            ts_available: 1_761_600_000_000_000_000 + seq as i64 * 1_000_000,
            source: None,
            source_seq: None,
            ingest_seq: seq,
            provenance: proto::Provenance::Live as i32,
            payload: Some(envelope::Payload::Heartbeat(Heartbeat {})),
        }
    }

    fn temp_root(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bender-store-crash-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    const SRC: &str = "CLOCK";

    /// Escribe 1..=n, simula el corte indicado, reabre, sigue hasta `total` y verifica.
    fn crash_and_resume(tag: &str, n: u64, crash: CrashPoint, truncate_wal_by: usize, total: u64) {
        let root = temp_root(tag);
        {
            let mut store = Store::open(&root).unwrap();
            let mut w = store.normalized_writer(SRC, "norm-1", WriterOptions { max_rows: 7 });
            for seq in 1..=n {
                w.append_envelope(&env(seq)).unwrap();
            }
            if crash != CrashPoint::None {
                w.close_with(crash).unwrap();
            }
            // El proceso "muere": el escritor se descarta sin cerrar.
        }
        let wal = root.join("normalized").join(source_dir(SRC)).join(WAL_NAME);
        if truncate_wal_by > 0 && wal.exists() {
            let len = std::fs::metadata(&wal).unwrap().len();
            let file = std::fs::OpenOptions::new().write(true).open(&wal).unwrap();
            file.set_len(len.saturating_sub(truncate_wal_by as u64))
                .unwrap();
        }

        let mut store = Store::open(&root).unwrap();
        let resume_from = store.last_ingest_seq(Zone::Normalized, SRC).unwrap_or(0);
        assert!(resume_from <= n);
        {
            let mut w = store.normalized_writer(SRC, "norm-1", WriterOptions { max_rows: 7 });
            for seq in resume_from + 1..=total {
                w.append_envelope(&env(seq)).unwrap();
            }
            w.close().unwrap();
        }
        // Reabrir otra vez no cambia nada (recuperación idempotente).
        let store = Store::open(&root).unwrap();
        let seqs: Vec<u64> = store
            .read_normalized(SRC, i64::MIN, i64::MAX)
            .unwrap()
            .iter()
            .map(|e| e.ingest_seq)
            .collect();
        assert_eq!(seqs, (1..=total).collect::<Vec<_>>(), "{tag}");
        // No quedan temporales, logs ni Parquet huérfanos.
        let mut files = Vec::new();
        walk(&root.join("normalized"), &mut files).unwrap();
        assert!(
            files
                .iter()
                .all(|f| f.extension().is_some_and(|e| e == "parquet")),
            "{files:?}"
        );
        assert_eq!(files.len(), store.segments().len());
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn crash_with_open_segment_recovers_the_valid_prefix() {
        // Sin cerrar: el log tiene 1..=10 (7 ya cerrados en un segmento por max_rows).
        crash_and_resume("open", 10, CrashPoint::None, 0, 25);
        // Log con el último registro cortado a la mitad: se descarta sólo ese registro.
        for cut in [1, 5, 13, 24, 30] {
            crash_and_resume(&format!("trunc{cut}"), 10, CrashPoint::None, cut, 25);
        }
    }

    #[test]
    fn crash_at_every_step_of_a_close() {
        for (i, point) in [
            CrashPoint::AfterTmp,
            CrashPoint::AfterRename,
            CrashPoint::AfterManifest,
        ]
        .into_iter()
        .enumerate()
        {
            crash_and_resume(&format!("close{i}"), 10, point, 0, 25);
        }
    }

    #[test]
    fn corrupt_manifest_tail_is_dropped() {
        let root = temp_root("manifest");
        {
            let mut store = Store::open(&root).unwrap();
            let mut w = store.normalized_writer(SRC, "norm-1", WriterOptions::default());
            for seq in 1..=3 {
                w.append_envelope(&env(seq)).unwrap();
            }
            w.close().unwrap();
        }
        let manifest = root.join("manifest.jsonl");
        let mut text = std::fs::read_to_string(&manifest).unwrap();
        text.push_str("{\"zone\":\"normalized\",\"sou");
        std::fs::write(&manifest, text).unwrap();
        let store = Store::open(&root).unwrap();
        assert_eq!(store.segments().len(), 1);
        assert_eq!(
            store
                .read_normalized(SRC, i64::MIN, i64::MAX)
                .unwrap()
                .len(),
            3
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
}
