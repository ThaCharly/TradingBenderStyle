//! Modo `dataset`: filas de features (de un [`Consumer`], que sólo ve el pasado) más
//! etiquetas (de un [`Labeler`], que recibe explícitamente una [`FutureWindow`]).

use std::path::Path;
use std::sync::Arc;

use arrow_array::{ArrayRef, Float64Array, Int64Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use bender_kernel::KernelConfig;
use bender_kernel::canonical::{CheckpointError, Checkpointable, Decoder, Encoder};
use bender_types::proto;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;

use crate::consumer::{Consumer, FutureWindow, Labeler};
use crate::{RunError, Runner};

/// Fila de features. Los floats existen sólo en features analíticas (doc 01 §1).
#[derive(Clone, Debug, PartialEq)]
pub struct DatasetRow {
    /// `ts_available` del evento en que se calculó.
    pub ts_available: i64,
    pub ingest_seq: u64,
    /// Nombre → valor. Todas las filas de un dataset tienen los mismos nombres.
    pub features: Vec<(String, f64)>,
    /// Etiqueta (la completa el [`Labeler`]).
    pub label: Option<f64>,
}

impl Checkpointable for DatasetRow {
    fn encode(&self, e: &mut Encoder) {
        let features: Vec<(String, u64)> = self
            .features
            .iter()
            .map(|(n, v)| (n.clone(), v.to_bits()))
            .collect();
        e.put(&self.ts_available)
            .put(&self.ingest_seq)
            .put(&features)
            .put(&self.label.map(f64::to_bits));
    }
    fn decode(d: &mut Decoder<'_>) -> Result<Self, CheckpointError> {
        let ts_available = d.get()?;
        let ingest_seq = d.get()?;
        let features: Vec<(String, u64)> = d.get()?;
        let label: Option<u64> = d.get()?;
        Ok(Self {
            ts_available,
            ingest_seq,
            features: features
                .into_iter()
                .map(|(n, b)| (n, f64::from_bits(b)))
                .collect(),
            label: label.map(f64::from_bits),
        })
    }
}

/// Corre el consumidor sobre `events` (en orden K) y etiqueta cada fila con el `labeler`
/// mirando la ventana `(ts_fila, ts_fila + horizonte]`.
pub fn dataset<C, L>(
    config: KernelConfig,
    consumer: C,
    labeler: &mut L,
    events: &[proto::Envelope],
) -> Result<Vec<DatasetRow>, RunError>
where
    C: Consumer<Output = DatasetRow>,
    L: Labeler<Row = DatasetRow>,
{
    let mut runner = Runner::new(config, consumer);
    let mut rows = Vec::new();
    for e in events {
        runner.process(e, &mut rows)?;
    }
    let horizon = labeler.horizon_ns();
    for row in &mut rows {
        let start = events.partition_point(|e| e.ts_available <= row.ts_available);
        let end_ts = row.ts_available.saturating_add(horizon);
        let end = events.partition_point(|e| e.ts_available <= end_ts);
        let window = FutureWindow::new(&events[start..end], row.ts_available, end_ts);
        row.label = Some(labeler.label(row, row.ts_available, &window));
    }
    Ok(rows)
}

/// Escribe un dataset en Parquet (zstd): `ts_available`, `ingest_seq`, una columna por
/// feature y `label`.
pub fn write_dataset_parquet(rows: &[DatasetRow], path: &Path) -> Result<(), String> {
    let names: Vec<String> = rows
        .first()
        .map(|r| r.features.iter().map(|(n, _)| n.clone()).collect())
        .unwrap_or_default();
    let mut fields = vec![
        Field::new("ts_available", DataType::Int64, false),
        Field::new("ingest_seq", DataType::UInt64, false),
    ];
    fields.extend(
        names
            .iter()
            .map(|n| Field::new(n, DataType::Float64, false)),
    );
    fields.push(Field::new("label", DataType::Float64, true));
    let schema = Arc::new(Schema::new(fields));
    let mut columns: Vec<ArrayRef> = vec![
        Arc::new(Int64Array::from_iter_values(
            rows.iter().map(|r| r.ts_available),
        )),
        Arc::new(UInt64Array::from_iter_values(
            rows.iter().map(|r| r.ingest_seq),
        )),
    ];
    for (i, name) in names.iter().enumerate() {
        let values: Result<Vec<f64>, String> = rows
            .iter()
            .map(|r| match r.features.get(i) {
                Some((n, v)) if n == name => Ok(*v),
                _ => Err(format!("fila sin la feature {name}")),
            })
            .collect();
        columns.push(Arc::new(Float64Array::from(values?)));
    }
    columns.push(Arc::new(Float64Array::from(
        rows.iter().map(|r| r.label).collect::<Vec<_>>(),
    )));
    let batch = RecordBatch::try_new(schema.clone(), columns).map_err(|e| e.to_string())?;
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::default()))
        .build();
    let file = std::fs::File::create(path).map_err(|e| e.to_string())?;
    let mut writer = ArrowWriter::try_new(file, schema, Some(props)).map_err(|e| e.to_string())?;
    writer.write(&batch).map_err(|e| e.to_string())?;
    writer.close().map_err(|e| e.to_string())?;
    Ok(())
}
