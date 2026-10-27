//! Tipos base del dominio: los mensajes generados del esquema (`proto`) y los tipos con
//! comportamiento que el resto del sistema usa en vez de los crudos: [`Decimal`],
//! [`Ulid`], [`TimestampNs`] y [`DurationNs`].
//!
//! Este crate no lee el reloj del sistema (Pilar 2): `ulid` se usa sin `std`, así que no
//! existe `Ulid::new()`.

mod decimal;
mod id;
mod time;

pub use decimal::{Decimal, DecimalError, MAX_SCALE};
pub use id::{Ulid, UlidError};
pub use time::{DurationNs, TimestampNs};

/// Mensajes generados desde `schema/bender/v1` con `make gen`. No editar a mano.
#[allow(clippy::all, clippy::pedantic, missing_docs)]
pub mod proto {
    include!("gen/bender.v1.rs");
}

/// Paquete del esquema que implementa este crate.
pub const SCHEMA_PACKAGE: &str = "bender.v1";
