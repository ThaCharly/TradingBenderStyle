//! Fixtures binarios para el test de ida y vuelta entre lenguajes (#19). Rust los
//! produce; `@bender/types` (TS) los decodifica y re-codifica a bytes idénticos.
//!
//! Para regenerarlos después de cambiar el esquema o este archivo:
//! `BENDER_UPDATE_FIXTURES=1 cargo test -p bender-types --test fixtures`.

use std::path::PathBuf;

use bender_types::proto::{self, envelope, market_event, source_id};
use bender_types::{Decimal, Ulid};
use prost::Message;

fn dec(s: &str) -> Option<proto::Decimal> {
    Some(s.parse::<Decimal>().unwrap().into())
}

fn id(n: u64) -> Option<proto::Ulid> {
    Some(Ulid::from_parts(1_761_600_000_000 + n, u128::from(n) * 0x9e37_79b9).into())
}

fn btc_perp() -> Option<proto::InstrumentId> {
    Some(proto::InstrumentId {
        venue: proto::Venue::Binance as i32,
        market_type: proto::MarketType::PerpLinear as i32,
        symbol: "BTCUSDT".into(),
    })
}

fn account() -> Option<proto::AccountId> {
    Some(proto::AccountId {
        venue: proto::Venue::Binance as i32,
        account: "principal".into(),
        wallet: "futures".into(),
    })
}

fn envelope_trade() -> proto::Envelope {
    proto::Envelope {
        ts_event: 1_761_600_000_123_000_000,
        ts_received: Some(1_761_600_000_125_500_000),
        ts_available: 1_761_600_000_125_500_000,
        source: Some(proto::SourceId {
            kind: Some(source_id::Kind::Market(proto::MarketSource {
                instrument: btc_perp(),
                data_kind: proto::MarketDataKind::Trades as i32,
            })),
        }),
        source_seq: Some(6_123_456_789),
        ingest_seq: 42,
        provenance: proto::Provenance::Live as i32,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(market_event::Event::Trade(proto::Trade {
                price: dec("88123.40"),
                qty_base: dec("0.012"),
                aggressor_side: proto::Side::Sell as i32,
                venue_trade_id: Some("6123456789".into()),
            })),
        })),
    }
}

fn order() -> proto::Order {
    proto::Order {
        order_id: id(1),
        client_order_id: "bdr-7f3a-sl-1".into(),
        venue_order_id: Some("401234567890".into()),
        intent_id: id(2),
        group_id: id(3),
        role: proto::OrderRole::StopLoss as i32,
        instrument: btc_perp(),
        account: account(),
        side: proto::Side::Sell as i32,
        position_side: proto::PositionSide::Net as i32,
        r#type: proto::OrderType::StopMarket as i32,
        time_in_force: proto::TimeInForce::Gtc as i32,
        price: None,
        trigger_price: dec("86000"),
        trigger_source: proto::TriggerSource::Mark as i32,
        qty_base: dec("0.030"),
        reduce_only: true,
        close_position: false,
        residence: proto::OrderResidence::VenueNative as i32,
        status: proto::OrderStatus::Accepted as i32,
    }
}

fn fill() -> proto::Fill {
    proto::Fill {
        fill_id: id(4),
        order_id: id(1),
        venue_trade_id: Some("998877".into()),
        price: dec("87999.9"),
        qty_base: dec("0.010"),
        fee: Some(proto::Fee {
            amount: dec("-0.00351999"),
            asset: Some(proto::Asset {
                code: "USDT".into(),
            }),
        }),
        liquidity: proto::Liquidity::Maker as i32,
        ts_event: 1_761_600_100_000_000_000,
        ts_received: 1_761_600_100_004_000_000,
    }
}

fn ledger_entry() -> proto::LedgerEntry {
    // Mantisa que no cabe en 64 bits, para ejercitar `mantissa_hi`.
    let big = Decimal::new(-(1i128 << 70) - 12_345, 18).unwrap();
    proto::LedgerEntry {
        entry_id: id(5),
        account: account(),
        r#type: proto::LedgerEntryType::Funding as i32,
        asset: Some(proto::Asset {
            code: "USDT".into(),
        }),
        amount: Some(big.into()),
        ts_event: 1_761_609_600_000_000_000,
        instrument: btc_perp(),
        fill_id: None,
        venue_ref_id: Some("funding-2026-10-28T00".into()),
    }
}

fn fixtures() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("envelope_trade.bin", envelope_trade().encode_to_vec()),
        ("order.bin", order().encode_to_vec()),
        ("fill.bin", fill().encode_to_vec()),
        ("ledger_entry.bin", ledger_entry().encode_to_vec()),
    ]
}

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[test]
fn fixtures_are_up_to_date() {
    let update = std::env::var_os("BENDER_UPDATE_FIXTURES").is_some();
    for (name, bytes) in fixtures() {
        let path = dir().join(name);
        if update {
            std::fs::create_dir_all(dir()).unwrap();
            std::fs::write(&path, &bytes).unwrap();
        }
        let stored = std::fs::read(&path).unwrap_or_else(|e| {
            panic!("falta {name} ({e}); regenerar con BENDER_UPDATE_FIXTURES=1")
        });
        assert_eq!(stored, bytes, "{name} desactualizado");
    }
}

#[test]
fn fixtures_decode_back_in_rust() {
    assert_eq!(
        proto::Envelope::decode(&*envelope_trade().encode_to_vec()).unwrap(),
        envelope_trade()
    );
    let entry = proto::LedgerEntry::decode(&*ledger_entry().encode_to_vec()).unwrap();
    let amount = Decimal::try_from(entry.amount.unwrap()).unwrap();
    assert_eq!(amount.mantissa(), -(1i128 << 70) - 12_345);
}
