//! Adaptador de Binance para datos públicos (spot y USDⓈ-M perps).
//!
//! - [`normalize`]: normalizador **puro** (sin E/S ni reloj): mensaje crudo del WS o del
//!   REST → eventos canónicos. Versionado con [`NORMALIZER_VERSION`]; si cambia la salida
//!   para un mismo crudo, cambia la versión.
//! - [`book::LocalBook`]: libro local que sigue las reglas de sincronización de Binance
//!   (snapshot REST + deltas con verificación de secuencia) y emite deltas canónicos
//!   restringidos al top-N.
//!
//! Las cantidades de Binance spot y USDⓈ-M ya vienen en unidades del activo base, así que
//! no hay conversión de contratos.

pub mod book;
pub mod normalize;
pub mod render;

pub use normalize::{
    Market, NormalizeError, Normalized, Parsed, normalize, open_interest, parse,
    parse_depth_snapshot, parse_rest_agg_trades, rest_agg_trade,
};

/// Versión del normalizador. Se graba con cada segmento normalizado.
pub const NORMALIZER_VERSION: &str = "binance-1";

use bender_types::proto;

/// Fuente canónica de un instrumento de Binance y un tipo de dato.
pub fn source(market: Market, symbol: &str, kind: proto::MarketDataKind) -> proto::SourceId {
    proto::SourceId {
        kind: Some(proto::source_id::Kind::Market(proto::MarketSource {
            instrument: Some(proto::InstrumentId {
                venue: proto::Venue::Binance as i32,
                market_type: match market {
                    Market::Spot => proto::MarketType::Spot,
                    Market::UsdM => proto::MarketType::PerpLinear,
                } as i32,
                symbol: symbol.to_ascii_uppercase(),
            }),
            data_kind: kind as i32,
        })),
    }
}
