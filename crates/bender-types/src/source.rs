//! Forma de texto canónica de una fuente (doc 05 §1):
//! `BINANCE:PERP_LINEAR:BTCUSDT:trades`, `USER:drawings`, `CLOCK`.

use crate::proto::{self, source_id};

/// Clave de texto de una fuente. Es estable: se usa en rutas de archivos, manifiestos y
/// la tabla `source_rank`. Un enum desconocido se escribe como `?<número>`.
pub fn source_key(source: &proto::SourceId) -> String {
    match &source.kind {
        Some(source_id::Kind::Market(m)) => {
            let (venue, market_type, symbol) = match &m.instrument {
                Some(i) => (
                    enum_name(
                        proto::Venue::try_from(i.venue)
                            .ok()
                            .map(|v| v.as_str_name()),
                        "VENUE_",
                        i.venue,
                    ),
                    enum_name(
                        proto::MarketType::try_from(i.market_type)
                            .ok()
                            .map(|v| v.as_str_name()),
                        "MARKET_TYPE_",
                        i.market_type,
                    ),
                    i.symbol.as_str(),
                ),
                None => ("?".into(), "?".into(), ""),
            };
            let kind = enum_name(
                proto::MarketDataKind::try_from(m.data_kind)
                    .ok()
                    .map(|v| v.as_str_name()),
                "MARKET_DATA_KIND_",
                m.data_kind,
            )
            .to_ascii_lowercase();
            format!("{venue}:{market_type}:{symbol}:{kind}")
        }
        Some(source_id::Kind::User(u)) => format!("USER:{}", u.stream),
        Some(source_id::Kind::Clock(_)) => "CLOCK".into(),
        None => "?".into(),
    }
}

fn enum_name(name: Option<&'static str>, prefix: &str, value: i32) -> String {
    match name {
        Some(n) => n.strip_prefix(prefix).unwrap_or(n).to_string(),
        None => format!("?{value}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_keys() {
        let market = proto::SourceId {
            kind: Some(source_id::Kind::Market(proto::MarketSource {
                instrument: Some(proto::InstrumentId {
                    venue: proto::Venue::Binance as i32,
                    market_type: proto::MarketType::PerpLinear as i32,
                    symbol: "BTCUSDT".into(),
                }),
                data_kind: proto::MarketDataKind::Trades as i32,
            })),
        };
        assert_eq!(source_key(&market), "BINANCE:PERP_LINEAR:BTCUSDT:trades");
        let user = proto::SourceId {
            kind: Some(source_id::Kind::User(proto::UserSource {
                stream: "drawings".into(),
            })),
        };
        assert_eq!(source_key(&user), "USER:drawings");
        let clock = proto::SourceId {
            kind: Some(source_id::Kind::Clock(proto::ClockSource {})),
        };
        assert_eq!(source_key(&clock), "CLOCK");
    }
}
