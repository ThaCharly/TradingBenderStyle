//! Transporte entre componentes (ADR-0005, doc 07 §2).
//!
//! - **Una interfaz, dos transportes**: los clientes generados por tonic se usan siempre
//!   sobre [`Channel`], que puede ser [`Channel::in_process`] (el servicio vive en el mismo
//!   binario; no hay red, pero sí serialización, para probar el mismo camino) o
//!   [`Channel::connect`] (HTTP/2, TLS opcional; en producción, sobre Tailscale).
//! - **Servidor**: [`server::serve`] expone los servicios por gRPC (HTTP/2) y gRPC-Web
//!   (HTTP/1.1, para el front con Connect-Web).
//! - **Versionado** ([`version`]): el cliente manda su versión en cada llamada; si es
//!   incompatible, el servidor responde un error que el cliente convierte en
//!   [`TransportError::IncompatibleVersion`].
//! - **Streams reanudables** ([`resume`]): ante un corte, el cliente se reconecta con el
//!   último cursor (`ingest_seq`) y no pierde ni duplica eventos.

pub mod example;
pub mod resume;
pub mod server;
pub mod version;

mod channel;
mod error;

pub use channel::{Channel, ClientTls};
pub use error::TransportError;
pub use version::{Compatibility, Version};

/// Error genérico de las capas de tower.
pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

/// Código generado desde `schema/bender/example/v1` con `make gen`. No editar a mano.
#[allow(missing_docs, clippy::large_enum_variant)]
pub mod proto {
    pub mod example {
        include!("gen/bender.example.v1.rs");
    }
}
