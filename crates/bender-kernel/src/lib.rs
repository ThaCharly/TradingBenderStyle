//! Núcleo temporal (doc 05): una sola librería con todas las reglas de tiempo del sistema.
//!
//! Este crate no lee el reloj del sistema (Pilar 2): el tiempo es el `ts_available` del
//! evento que se procesa. Lo garantizan el `clippy.toml` del crate y `tools/arch-tests`.

pub mod merge;
pub mod order;

pub use merge::{Merge, MergeError};
pub use order::{Ordered, SourceOrder, SourceRankTable, UnknownSource, compare};
