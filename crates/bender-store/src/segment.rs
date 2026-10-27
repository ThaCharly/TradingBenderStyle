//! Escritura y lectura de un segmento Parquet, y su hash de contenido canónico.

use std::sync::Arc;

use arrow_array::builder::{BinaryBuilder, Int64Builder, StringBuilder, UInt64Builder};
use arrow_array::{Array, ArrayRef, BinaryArray, Int64Array, RecordBatch, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use bender_types::proto::{self, envelope, market_event};
use parquet::arrow::ArrowWriter;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::basic::{Compression, ZstdLevel};
use parquet::file::properties::WriterProperties;
use prost::Message;

use crate::{StoreError, Zone};

/// Fila genérica: en la zona normalizada `data` es el `Envelope` codificado y `ts` su
/// `ts_available`; en la cruda, `data` es el mensaje original y `ts` su `ts_received`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub ingest_seq: u64,
    pub ts: i64,
    pub data: Vec<u8>,
}

/// Registro de la zona cruda.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RawRecord {
    /// ns UTC.
    pub ts_received: i64,
    pub ingest_seq: u64,
    /// Bytes del mensaje WS/REST tal cual llegó.
    pub payload: Vec<u8>,
}

/// Hash de contenido: BLAKE3 de una serialización canónica que no depende de Parquet ni
/// de la máquina. Cubre zona, fuente, versión del normalizador y cada fila en orden.
pub fn content_hash(zone: Zone, source: &str, normalizer_version: &str, rows: &[Row]) -> String {
    let mut h = blake3::Hasher::new();
    h.update(b"bender-store/segment/1\0");
    h.update(zone.dir_name().as_bytes());
    h.update(b"\0");
    h.update(source.as_bytes());
    h.update(b"\0");
    h.update(normalizer_version.as_bytes());
    h.update(b"\0");
    for row in rows {
        h.update(&row.ingest_seq.to_be_bytes());
        h.update(&row.ts.to_be_bytes());
        h.update(&(row.data.len() as u64).to_be_bytes());
        h.update(&row.data);
    }
    h.finalize().to_hex().to_string()
}

fn payload_kind(env: &proto::Envelope) -> &'static str {
    match &env.payload {
        Some(envelope::Payload::Market(m)) => match &m.event {
            Some(market_event::Event::Trade(_)) => "trade",
            Some(market_event::Event::BookSnapshot(_)) => "book_snapshot",
            Some(market_event::Event::BookDelta(_)) => "book_delta",
            Some(market_event::Event::TopOfBook(_)) => "top_of_book",
            Some(market_event::Event::MarkPrice(_)) => "mark_price",
            Some(market_event::Event::IndexPrice(_)) => "index_price",
            Some(market_event::Event::Funding(_)) => "funding",
            Some(market_event::Event::OpenInterest(_)) => "open_interest",
            Some(market_event::Event::Liquidation(_)) => "liquidation",
            Some(market_event::Event::InstrumentStatus(_)) => "instrument_status",
            None => "market",
        },
        Some(envelope::Payload::User(_)) => "user",
        Some(envelope::Payload::Heartbeat(_)) => "heartbeat",
        Some(envelope::Payload::Gap(_)) => "gap",
        Some(envelope::Payload::GapRecovery(_)) => "gap_recovery",
        None => "none",
    }
}

fn provenance_name(value: i32) -> String {
    proto::Provenance::try_from(value)
        .map(|p| {
            p.as_str_name()
                .trim_start_matches("PROVENANCE_")
                .to_ascii_lowercase()
        })
        .unwrap_or_else(|_| format!("?{value}"))
}

fn schema(zone: Zone) -> Arc<Schema> {
    let fields = match zone {
        Zone::Normalized => vec![
            Field::new("source", DataType::Utf8, false),
            Field::new("ts_event", DataType::Int64, false),
            Field::new("ts_received", DataType::Int64, true),
            Field::new("ts_available", DataType::Int64, false),
            Field::new("source_seq", DataType::UInt64, true),
            Field::new("ingest_seq", DataType::UInt64, false),
            Field::new("provenance", DataType::Utf8, false),
            Field::new("payload_kind", DataType::Utf8, false),
            Field::new("normalizer_version", DataType::Utf8, false),
            Field::new("envelope", DataType::Binary, false),
        ],
        Zone::Raw => vec![
            Field::new("source", DataType::Utf8, false),
            Field::new("ts_received", DataType::Int64, false),
            Field::new("ingest_seq", DataType::UInt64, false),
            Field::new("payload", DataType::Binary, false),
        ],
    };
    Arc::new(Schema::new(fields))
}

fn batch(
    zone: Zone,
    source: &str,
    normalizer_version: &str,
    rows: &[Row],
) -> Result<RecordBatch, StoreError> {
    let mut source_col = StringBuilder::new();
    let mut ingest = UInt64Builder::new();
    let mut data = BinaryBuilder::new();
    for row in rows {
        source_col.append_value(source);
        ingest.append_value(row.ingest_seq);
        data.append_value(&row.data);
    }
    let columns: Vec<ArrayRef> = match zone {
        Zone::Raw => {
            let ts = Int64Array::from_iter_values(rows.iter().map(|r| r.ts));
            vec![
                Arc::new(source_col.finish()),
                Arc::new(ts),
                Arc::new(ingest.finish()),
                Arc::new(data.finish()),
            ]
        }
        Zone::Normalized => {
            let mut ts_event = Int64Builder::new();
            let mut ts_received = Int64Builder::new();
            let mut source_seq = UInt64Builder::new();
            let mut provenance = StringBuilder::new();
            let mut kind = StringBuilder::new();
            let mut version = StringBuilder::new();
            for row in rows {
                let env = proto::Envelope::decode(row.data.as_slice())
                    .map_err(|e| StoreError::Decode(e.to_string()))?;
                ts_event.append_value(env.ts_event);
                ts_received.append_option(env.ts_received);
                source_seq.append_option(env.source_seq);
                provenance.append_value(provenance_name(env.provenance));
                kind.append_value(payload_kind(&env));
                version.append_value(normalizer_version);
            }
            let ts_available = Int64Array::from_iter_values(rows.iter().map(|r| r.ts));
            vec![
                Arc::new(source_col.finish()),
                Arc::new(ts_event.finish()),
                Arc::new(ts_received.finish()),
                Arc::new(ts_available),
                Arc::new(source_seq.finish()),
                Arc::new(ingest.finish()),
                Arc::new(provenance.finish()),
                Arc::new(kind.finish()),
                Arc::new(version.finish()),
                Arc::new(data.finish()),
            ]
        }
    };
    Ok(RecordBatch::try_new(schema(zone), columns)?)
}

const BATCH_ROWS: usize = 65_536;

/// Codifica las filas como Parquet (zstd).
pub fn write_parquet(
    zone: Zone,
    source: &str,
    normalizer_version: &str,
    rows: &[Row],
) -> Result<Vec<u8>, StoreError> {
    let props = WriterProperties::builder()
        .set_compression(Compression::ZSTD(ZstdLevel::try_new(3)?))
        .set_created_by("bender-store".into())
        .build();
    let mut out = Vec::new();
    let mut writer = ArrowWriter::try_new(&mut out, schema(zone), Some(props))?;
    for chunk in rows.chunks(BATCH_ROWS) {
        writer.write(&batch(zone, source, normalizer_version, chunk)?)?;
    }
    writer.close()?;
    Ok(out)
}

/// Lee las filas de un segmento en el orden en que se escribieron.
pub fn read_parquet(zone: Zone, bytes: Vec<u8>) -> Result<Vec<Row>, StoreError> {
    let (ts_col, data_col) = match zone {
        Zone::Normalized => ("ts_available", "envelope"),
        Zone::Raw => ("ts_received", "payload"),
    };
    let reader = ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(bytes))?.build()?;
    let mut rows = Vec::new();
    for batch in reader {
        let batch = batch?;
        let column = |name: &str| {
            batch
                .column_by_name(name)
                .ok_or_else(|| StoreError::Decode(format!("falta la columna {name}")))
        };
        let ingest = column("ingest_seq")?
            .as_any()
            .downcast_ref::<UInt64Array>()
            .ok_or_else(|| StoreError::Decode("ingest_seq no es UInt64".into()))?;
        let ts = column(ts_col)?
            .as_any()
            .downcast_ref::<Int64Array>()
            .ok_or_else(|| StoreError::Decode(format!("{ts_col} no es Int64")))?;
        let data = column(data_col)?
            .as_any()
            .downcast_ref::<BinaryArray>()
            .ok_or_else(|| StoreError::Decode(format!("{data_col} no es Binary")))?;
        for i in 0..batch.num_rows() {
            if ingest.is_null(i) || ts.is_null(i) || data.is_null(i) {
                return Err(StoreError::Decode(
                    "valor nulo en columna obligatoria".into(),
                ));
            }
            rows.push(Row {
                ingest_seq: ingest.value(i),
                ts: ts.value(i),
                data: data.value(i).to_vec(),
            });
        }
    }
    Ok(rows)
}
