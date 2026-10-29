//! Configuración del grabador (TOML). Todos los campos tienen default; el universo por
//! defecto es el de #32: BTC y ETH contra USDT y USDC, en spot y perps, con L2 sólo para
//! los USDT.

use serde::{Deserialize, Serialize};

use crate::core::{BookMode, CoreConfig, Instrument, Link};
use bender_types::proto::MarketDataKind;
use bender_venue_binance::Market;

/// Mercado en el archivo de configuración.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum MarketName {
    Spot,
    Usdm,
}

impl From<MarketName> for Market {
    fn from(m: MarketName) -> Self {
        match m {
            MarketName::Spot => Market::Spot,
            MarketName::Usdm => Market::UsdM,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct InstrumentConfig {
    pub market: MarketName,
    pub symbol: String,
    pub book: BookMode,
}

/// URLs de Binance (configurables para apuntar a un servidor simulado). USDⓈ-M usa dos
/// conexiones: `/public` (libro, `bookTicker`) y `/market` (trades, mark price,
/// liquidaciones); la ruta sin sufijo ya no entrega trades (verificado con capturas
/// reales, 2026-10).
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Endpoints {
    pub spot_ws: String,
    pub spot_rest: String,
    pub usdm_ws_public: String,
    pub usdm_ws_market: String,
    pub usdm_rest: String,
}

impl Default for Endpoints {
    fn default() -> Self {
        Self {
            spot_ws: "wss://stream.binance.com:9443".into(),
            spot_rest: "https://api.binance.com".into(),
            usdm_ws_public: "wss://fstream.binance.com/public".into(),
            usdm_ws_market: "wss://fstream.binance.com/market".into(),
            usdm_rest: "https://fapi.binance.com".into(),
        }
    }
}

impl Endpoints {
    pub fn ws(&self, link: Link) -> &str {
        match link {
            Link::Spot => &self.spot_ws,
            Link::UsdmPublic => &self.usdm_ws_public,
            Link::UsdmMarket => &self.usdm_ws_market,
        }
    }

    pub fn rest(&self, market: Market) -> &str {
        match market {
            Market::Spot => &self.spot_rest,
            Market::UsdM => &self.usdm_rest,
        }
    }
}

/// Límites de tasa del REST: peso por minuto que el grabador se permite usar, por
/// mercado. Binance permite 6000 (spot) y 2400 (USDⓈ-M) por IP; por defecto se usa la
/// mitad para dejar margen a otros procesos de la misma IP.
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct RateLimits {
    pub spot_weight_per_minute: u32,
    pub usdm_weight_per_minute: u32,
    /// Fracción del presupuesto que puede usar la recuperación (prioridad baja), en %.
    pub recovery_share_percent: u32,
}

impl Default for RateLimits {
    fn default() -> Self {
        Self {
            spot_weight_per_minute: 3000,
            usdm_weight_per_minute: 1200,
            recovery_share_percent: 50,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub instruments: Vec<InstrumentConfig>,
    /// Niveles por lado del libro L2 (ADR-0009).
    pub top_n: usize,
    /// Profundidad del snapshot REST (Binance: 5, 10, 20, 50, 100, 500, 1000).
    pub snapshot_depth: u32,
    pub book_snapshot_every_s: u64,
    pub heartbeat_ms: u64,
    pub stale_after_s: u64,
    pub open_interest_every_s: u64,
    pub recovery_page: u32,
    pub max_recovery_pages: u32,
    /// Filas y MiB de datos como máximo por segmento del store (cada segmento abierto vive
    /// en memoria: con ~40 fuentes abiertas, el tope de memoria es ~40 × `segment_mib`).
    pub segment_rows: usize,
    pub segment_mib: usize,
    /// Reconexión: espera inicial y máxima, en ms.
    pub reconnect_min_ms: u64,
    pub reconnect_max_ms: u64,
    pub endpoints: Endpoints,
    pub rate_limits: RateLimits,
}

impl Default for Config {
    fn default() -> Self {
        let mut instruments = Vec::new();
        for market in [MarketName::Spot, MarketName::Usdm] {
            for symbol in ["BTCUSDT", "ETHUSDT", "BTCUSDC", "ETHUSDC"] {
                instruments.push(InstrumentConfig {
                    market,
                    symbol: symbol.into(),
                    book: if symbol.ends_with("USDT") {
                        BookMode::L2
                    } else {
                        BookMode::Top
                    },
                });
            }
        }
        Self {
            instruments,
            top_n: 20,
            snapshot_depth: 1000,
            book_snapshot_every_s: 60,
            heartbeat_ms: 100,
            stale_after_s: 10,
            open_interest_every_s: 60,
            recovery_page: 1000,
            max_recovery_pages: 5000,
            segment_rows: 200_000,
            segment_mib: 16,
            reconnect_min_ms: 1_000,
            reconnect_max_ms: 60_000,
            endpoints: Endpoints::default(),
            rate_limits: RateLimits::default(),
        }
    }
}

impl Config {
    pub fn from_toml(text: &str) -> Result<Self, String> {
        let c: Self = toml::from_str(text).map_err(|e| e.to_string())?;
        if c.top_n == 0 || c.heartbeat_ms == 0 || c.recovery_page == 0 {
            return Err("top_n, heartbeat_ms y recovery_page tienen que ser positivos".into());
        }
        if c.recovery_page > 1000 {
            return Err("recovery_page: Binance devuelve como máximo 1000".into());
        }
        Ok(c)
    }

    pub fn core(&self) -> CoreConfig {
        CoreConfig {
            instruments: self
                .instruments
                .iter()
                .map(|i| Instrument {
                    market: i.market.into(),
                    symbol: i.symbol.to_ascii_uppercase(),
                    book: i.book,
                })
                .collect(),
            top_n: self.top_n,
            book_snapshot_every_ns: self.book_snapshot_every_s as i64 * 1_000_000_000,
            stale_after_ns: self.stale_after_s as i64 * 1_000_000_000,
            open_interest_every_ns: self.open_interest_every_s as i64 * 1_000_000_000,
            recovery_page: self.recovery_page,
            max_recovery_pages: self.max_recovery_pages,
        }
    }

    /// Streams del WS combinado de una conexión.
    pub fn streams(&self, link: Link) -> Vec<String> {
        let mut out = Vec::new();
        for i in self
            .instruments
            .iter()
            .filter(|i| Market::from(i.market) == link.market())
        {
            out.extend(link_streams(link, &i.symbol, Some(i.book)));
        }
        out
    }
}

/// Streams de un símbolo en una conexión. `book = None`: los dos tipos de libro (para
/// capturar fixtures).
pub fn link_streams(link: Link, symbol: &str, book: Option<BookMode>) -> Vec<String> {
    let s = symbol.to_ascii_lowercase();
    let mut out = Vec::new();
    if link.carries(MarketDataKind::Trades) {
        out.push(format!("{s}@aggTrade"));
    }
    if link.carries(MarketDataKind::Book) {
        if book != Some(BookMode::Top) {
            out.push(format!("{s}@depth@100ms"));
        }
        if book != Some(BookMode::L2) {
            out.push(format!("{s}@bookTicker"));
        }
    }
    if link.market() == Market::UsdM && link.carries(MarketDataKind::MarkPrice) {
        out.push(format!("{s}@markPrice@1s"));
        out.push(format!("{s}@forceOrder"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_issue_32() {
        let c = Config::default();
        assert_eq!(c.instruments.len(), 8);
        assert_eq!(
            c.instruments
                .iter()
                .filter(|i| i.book == BookMode::L2)
                .count(),
            4
        );
        assert_eq!(c.top_n, 20);
        assert_eq!(c.heartbeat_ms, 100);
        let has = |l: Link, s: &str| c.streams(l).contains(&s.to_string());
        assert!(has(Link::UsdmPublic, "btcusdt@depth@100ms"));
        assert!(has(Link::UsdmMarket, "btcusdt@aggTrade"));
        assert!(has(Link::UsdmMarket, "btcusdt@markPrice@1s"));
        assert!(!has(Link::UsdmPublic, "btcusdt@aggTrade"));
        assert!(!has(Link::UsdmMarket, "btcusdt@depth@100ms"));
        assert!(has(Link::Spot, "btcusdc@bookTicker"));
        assert!(has(Link::Spot, "btcusdt@aggTrade"));
        assert!(
            !c.streams(Link::Spot)
                .iter()
                .any(|s| s.contains("markPrice"))
        );
    }

    #[test]
    fn toml_overrides_and_example_parses() {
        let c = Config::from_toml("top_n = 10\n[[instruments]]\nmarket = \"usdm\"\nsymbol = \"BTCUSDT\"\nbook = \"top\"\n").unwrap();
        assert_eq!(c.top_n, 10);
        assert_eq!(c.instruments.len(), 1);
        let example = include_str!("../recorder.example.toml");
        assert_eq!(Config::from_toml(example).unwrap(), Config::default());
        assert!(Config::from_toml("desconocido = 1").is_err());
    }
}
