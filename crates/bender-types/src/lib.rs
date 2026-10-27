//! Tipos base del dominio: los mensajes generados del esquema (`proto`) y los tipos con
//! comportamiento que el resto del sistema usa en vez de los crudos: [`Decimal`],
//! [`Ulid`], [`TimestampNs`] y [`DurationNs`].
//!
//! Este crate no lee el reloj del sistema (Pilar 2): `ulid` se usa sin `std`, así que no
//! existe `Ulid::new()`.

mod decimal;
mod id;
mod source;
mod time;

pub use decimal::{Decimal, DecimalError, MAX_SCALE};
pub use id::{Ulid, UlidError};
pub use source::source_key;
pub use time::{DurationNs, TimestampNs};

/// Mensajes generados desde `schema/bender/v1` con `make gen`. No editar a mano.
///
/// Se permiten lints puntuales y no grupos: `clippy::all` incluye `disallowed_methods`,
/// que es `forbid` en el workspace (Pilar 2).
#[allow(
    missing_docs,
    clippy::large_enum_variant,
    clippy::doc_lazy_continuation,
    clippy::doc_overindented_list_items
)]
pub mod proto {
    include!("gen/bender.v1.rs");
}

/// Paquete del esquema que implementa este crate.
pub const SCHEMA_PACKAGE: &str = "bender.v1";
