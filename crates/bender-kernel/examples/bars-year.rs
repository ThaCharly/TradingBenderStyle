//! Benchmark de #25: agrega un año de trades sintéticos de BTCUSDT en las 19
//! temporalidades estándar.
//!
//! El tiempo se mide desde afuera, porque este crate no lee el reloj (Pilar 2):
//! `cargo build --release -p bender-kernel --example bars-year` y después
//! `time target/release/examples/bars-year [trades_por_día] [días]`
//! (por defecto 1.500.000 trades/día durante 365 días, del orden de BTCUSDT perp en
//! Binance: ~550 M trades).

use bender_kernel::{BarAggregator, BarConfig, Timeframe, TradeTick};
use bender_types::Decimal;
use bender_types::proto::Side;

const DAY: i64 = 86_400_000_000_000;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let per_day: u64 = args
        .get(1)
        .map_or(1_500_000, |a| a.parse().expect("trades por día"));
    let days: u64 = args.get(2).map_or(365, |a| a.parse().expect("días"));
    let total = per_day * days;
    let step = DAY / per_day as i64;

    let mut agg = BarAggregator::new(
        "BINANCE:PERP_LINEAR:BTCUSDT:trades",
        &Timeframe::STANDARD,
        BarConfig::default(),
    );
    let mut events = Vec::new();
    let mut closed = 0u64;
    let mut rng = 0x5eed_u64;
    let mut price: i64 = 6_500_000; // 65.000,00
    let start_ts = 1_735_689_600_000_000_000i64; // 2025-01-01
    for i in 0..total {
        rng ^= rng << 13;
        rng ^= rng >> 7;
        rng ^= rng << 17;
        price += (rng % 21) as i64 - 10;
        let trade = TradeTick {
            ts_event: start_ts + i as i64 * step,
            price: Decimal::new(price.into(), 2).expect("precio"),
            qty_base: Decimal::new(i128::from(rng % 5_000 + 1), 3).expect("cantidad"),
            aggressor: if rng & 1 == 0 { Side::Buy } else { Side::Sell },
        };
        agg.on_trade(&trade, trade.ts_event, &mut events)
            .expect("trade");
        closed += events.len() as u64;
        events.clear();
    }
    agg.on_heartbeat(start_ts + (days as i64 + 400) * DAY, &mut events);
    closed += events.len() as u64;
    println!(
        "{total} trades ({per_day}/día × {days} días), 19 temporalidades, {closed} velas cerradas"
    );
}
