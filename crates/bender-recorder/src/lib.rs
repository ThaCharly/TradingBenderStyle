//! Grabador de datos públicos de Binance (spot + USDⓈ-M perps), ADR-0004 y ADR-0011.
//!
//! - [`core`]: la lógica, sin E/S (huecos, recuperación, libro, latidos).
//! - [`io`]: conexiones WS y REST, límites de tasa y escritura en `bender-store`.
//! - [`config`]: configuración (TOML) con los defaults de #32.
//! - [`report`]: reporte de una grabación (eventos por fuente, huecos, recuperaciones y
//!   tamaño en disco por tipo de dato).

pub mod capture;
pub mod config;
pub mod core;
pub mod io;
pub mod report;
