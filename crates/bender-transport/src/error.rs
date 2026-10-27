//! Errores tipados del transporte.

use std::fmt;

use bender_types::proto;
use prost::Message;
use tonic::{Code, Status};

use crate::version::{MISMATCH_TYPE_URL, RpcStatus};

/// Error de una llamada.
#[derive(Debug)]
pub enum TransportError {
    /// La versión del cliente no es compatible con la del servicio.
    IncompatibleVersion(proto::VersionMismatch),
    /// Cualquier otro error de la llamada.
    Rpc(Status),
}

impl TransportError {
    /// Si conviene reintentar (corte de red, servidor reiniciando…).
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::IncompatibleVersion(_) => false,
            Self::Rpc(status) => matches!(
                status.code(),
                Code::Unavailable
                    | Code::Unknown
                    | Code::Internal
                    | Code::Cancelled
                    | Code::Aborted
                    | Code::DeadlineExceeded
            ),
        }
    }
}

impl From<Status> for TransportError {
    fn from(status: Status) -> Self {
        if status.code() == Code::FailedPrecondition
            && let Ok(details) = RpcStatus::decode(status.details())
            && let Some(any) = details
                .details
                .iter()
                .find(|a| a.type_url == MISMATCH_TYPE_URL)
            && let Ok(mismatch) = proto::VersionMismatch::decode(any.value.as_slice())
        {
            return Self::IncompatibleVersion(mismatch);
        }
        Self::Rpc(status)
    }
}

impl fmt::Display for TransportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::IncompatibleVersion(m) => {
                let server = m.server.map(crate::Version::from);
                let client = m.client.map(crate::Version::from);
                write!(
                    f,
                    "{}: versión incompatible (servidor {}, cliente {})",
                    m.service,
                    server.map_or("?".into(), |v| v.to_string()),
                    client.map_or("sin versión".into(), |v| v.to_string()),
                )
            }
            Self::Rpc(status) => write!(f, "{status}"),
        }
    }
}

impl std::error::Error for TransportError {}
