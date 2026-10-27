//! Orden total de eventos (doc 05 §2):
//! `K = (ts_available, source_rank, source_seq, ingest_seq)`.
//!
//! - `source_rank` sale de la tabla versionada `source_rank.toml`.
//! - `source_seq` ausente ordena **antes** que cualquier valor presente (como `None <
//!   Some` en Rust).
//! - Dos fuentes distintas con el mismo `source_rank` (p. ej. dos instrumentos del mismo
//!   venue) pueden empatar en los cuatro campos. Para que el orden sea total, el último
//!   desempate es la clave canónica de la fuente (`bender_types::source_key`), en orden
//!   lexicográfico de bytes.

use std::cmp::Ordering;
use std::sync::OnceLock;

use bender_types::proto::{self, source_id};
use bender_types::source_key;
use serde::Deserialize;

/// Lo que el orden total necesita de un evento. La fuente se conoce por el stream al que
/// pertenece ([`SourceOrder`]).
pub trait Ordered {
    fn ts_available(&self) -> i64;
    fn source_seq(&self) -> Option<u64>;
    fn ingest_seq(&self) -> u64;
}

impl Ordered for proto::Envelope {
    fn ts_available(&self) -> i64 {
        self.ts_available
    }
    fn source_seq(&self) -> Option<u64> {
        self.source_seq
    }
    fn ingest_seq(&self) -> u64 {
        self.ingest_seq
    }
}

/// Posición de una fuente en el orden: su `source_rank` y su clave canónica.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SourceOrder {
    pub rank: u32,
    pub key: String,
}

/// Compara dos eventos según `K`, con la clave de la fuente como último desempate.
pub fn compare<A: Ordered, B: Ordered>(
    a: &A,
    sa: &SourceOrder,
    b: &B,
    sb: &SourceOrder,
) -> Ordering {
    a.ts_available()
        .cmp(&b.ts_available())
        .then(sa.rank.cmp(&sb.rank))
        .then(a.source_seq().cmp(&b.source_seq()))
        .then(a.ingest_seq().cmp(&b.ingest_seq()))
        .then_with(|| sa.key.cmp(&sb.key))
}

/// La fuente no tiene categoría en la tabla (venue desconocido o sin especificar).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownSource(pub String);

impl std::fmt::Display for UnknownSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "fuente sin source_rank: {}", self.0)
    }
}

impl std::error::Error for UnknownSource {}

/// Tabla `source_rank` versionada.
#[derive(Clone, Debug)]
pub struct SourceRankTable {
    version: u32,
    order: Vec<String>,
    content_hash: String,
}

#[derive(Deserialize)]
struct TableFile {
    version: u32,
    order: Vec<String>,
}

/// Tabla vigente, embebida en el binario.
const CURRENT: &str = include_str!("../source_rank.toml");

impl SourceRankTable {
    /// La tabla vigente (`crates/bender-kernel/source_rank.toml`).
    pub fn current() -> &'static Self {
        static TABLE: OnceLock<SourceRankTable> = OnceLock::new();
        TABLE.get_or_init(|| Self::parse(CURRENT).expect("source_rank.toml válido"))
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        let file: TableFile = toml::from_str(text).map_err(|e| e.to_string())?;
        let mut seen = std::collections::BTreeSet::new();
        if let Some(dup) = file.order.iter().find(|c| !seen.insert(c.as_str())) {
            return Err(format!("categoría repetida: {dup}"));
        }
        Ok(Self {
            version: file.version,
            order: file.order,
            content_hash: blake3::hash(text.as_bytes()).to_hex().to_string(),
        })
    }

    /// Versión de la tabla, para el manifiesto de corrida.
    pub fn version(&self) -> u32 {
        self.version
    }

    /// BLAKE3 del archivo de la tabla, para el manifiesto de corrida.
    pub fn content_hash(&self) -> &str {
        &self.content_hash
    }

    /// `source_rank` de una fuente según su categoría (`CLOCK`, `USER` o el venue).
    pub fn rank(&self, source: &proto::SourceId) -> Result<u32, UnknownSource> {
        let category = match &source.kind {
            Some(source_id::Kind::Clock(_)) => Some("CLOCK"),
            Some(source_id::Kind::User(_)) => Some("USER"),
            Some(source_id::Kind::Market(m)) => m
                .instrument
                .as_ref()
                .and_then(|i| proto::Venue::try_from(i.venue).ok())
                .filter(|v| *v != proto::Venue::Unspecified)
                .map(|v| v.as_str_name().trim_start_matches("VENUE_")),
            None => None,
        };
        category
            .and_then(|c| self.order.iter().position(|o| o == c))
            .map(|p| p as u32)
            .ok_or_else(|| UnknownSource(source_key(source)))
    }

    /// [`SourceOrder`] de una fuente.
    pub fn source_order(&self, source: &proto::SourceId) -> Result<SourceOrder, UnknownSource> {
        Ok(SourceOrder {
            rank: self.rank(source)?,
            key: source_key(source),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn market(venue: proto::Venue, symbol: &str) -> proto::SourceId {
        proto::SourceId {
            kind: Some(source_id::Kind::Market(proto::MarketSource {
                instrument: Some(proto::InstrumentId {
                    venue: venue as i32,
                    market_type: proto::MarketType::Spot as i32,
                    symbol: symbol.into(),
                }),
                data_kind: proto::MarketDataKind::Trades as i32,
            })),
        }
    }

    #[test]
    fn current_table_is_clock_user_then_venues_alphabetically() {
        let t = SourceRankTable::current();
        assert_eq!(t.version(), 1);
        assert_eq!(t.content_hash().len(), 64);
        let clock = proto::SourceId {
            kind: Some(source_id::Kind::Clock(proto::ClockSource {})),
        };
        let user = proto::SourceId {
            kind: Some(source_id::Kind::User(proto::UserSource {
                stream: "drawings".into(),
            })),
        };
        let ranks: Vec<u32> = [
            clock,
            user,
            market(proto::Venue::Binance, "BTCUSDT"),
            market(proto::Venue::Bingx, "BTC-USDT"),
            market(proto::Venue::Blofin, "BTC-USDT"),
            market(proto::Venue::Bybit, "BTCUSDT"),
            market(proto::Venue::Mexc, "BTCUSDT"),
            market(proto::Venue::Paper, "BTCUSDT"),
        ]
        .iter()
        .map(|s| t.rank(s).unwrap())
        .collect();
        assert_eq!(ranks, (0..8).collect::<Vec<_>>());
        assert!(t.rank(&market(proto::Venue::Unspecified, "X")).is_err());
    }

    #[test]
    fn rejects_duplicate_categories() {
        assert!(SourceRankTable::parse("version = 2\norder = [\"CLOCK\", \"CLOCK\"]").is_err());
    }
}
