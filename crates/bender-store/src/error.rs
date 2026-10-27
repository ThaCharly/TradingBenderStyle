use std::fmt;
use std::path::PathBuf;

/// Errores del store.
#[derive(Debug)]
pub enum StoreError {
    Io(std::io::Error),
    Parquet(String),
    /// Los bytes de un segmento cerrado no coinciden con su hash: el archivo se alteró.
    Integrity {
        path: PathBuf,
        expected: String,
        actual: String,
    },
    /// Un registro no se pudo decodificar.
    Decode(String),
    /// `ingest_seq` no creciente dentro de una fuente.
    OutOfOrder {
        source: String,
        last: u64,
        got: u64,
    },
    /// El manifiesto nombra un segmento que no existe.
    MissingSegment(PathBuf),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(e) => write!(f, "E/S: {e}"),
            Self::Parquet(e) => write!(f, "Parquet: {e}"),
            Self::Integrity {
                path,
                expected,
                actual,
            } => write!(
                f,
                "integridad: {} tiene hash {actual}, se esperaba {expected}",
                path.display()
            ),
            Self::Decode(e) => write!(f, "decodificación: {e}"),
            Self::OutOfOrder { source, last, got } => write!(
                f,
                "{source}: ingest_seq {got} no es mayor que el último ({last})"
            ),
            Self::MissingSegment(p) => write!(f, "falta el segmento {}", p.display()),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e)
    }
}

impl From<parquet::errors::ParquetError> for StoreError {
    fn from(e: parquet::errors::ParquetError) -> Self {
        Self::Parquet(e.to_string())
    }
}

impl From<arrow_schema::ArrowError> for StoreError {
    fn from(e: arrow_schema::ArrowError) -> Self {
        Self::Parquet(e.to_string())
    }
}
