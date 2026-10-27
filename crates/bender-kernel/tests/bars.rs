//! Criterios de #25: velas.

use std::collections::BTreeMap;

use bender_kernel::{
    Bar, BarAggregator, BarConfig, BarEvent, BarEventKind, BarState, Timeframe, TradeTick,
};
use bender_types::proto::Side;
use bender_types::{Decimal, days_from_civil};
use proptest::prelude::*;

const S: i64 = 1_000_000_000;
const DAY: i64 = 86_400 * S;
const SRC: &str = "BINANCE:PERP_LINEAR:BTCUSDT:trades";

fn ts(y: i64, m: u32, d: u32, h: i64, min: i64) -> i64 {
    days_from_civil(y, m, d) * DAY + h * 3_600 * S + min * 60 * S
}

// ---------------------------------------------------------------------------------------
// Calendario de referencia, independiente del del núcleo: cuenta años y meses a mano.

fn is_leap(y: i64) -> bool {
    (y % 4 == 0 && y % 100 != 0) || y % 400 == 0
}

fn month_len(y: i64, m: u32) -> i64 {
    match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        _ if is_leap(y) => 29,
        _ => 28,
    }
}

/// (año, mes, día de inicio del mes en días desde 1970) para un día ≥ 0.
fn naive_civil(mut days: i64) -> (i64, u32, i64) {
    let mut y = 1970;
    let mut base = 0;
    loop {
        let len = if is_leap(y) { 366 } else { 365 };
        if days < len {
            break;
        }
        days -= len;
        base += len;
        y += 1;
    }
    let mut m = 1;
    while days >= month_len(y, m) {
        days -= month_len(y, m);
        base += month_len(y, m);
        m += 1;
    }
    (y, m, base)
}

fn naive_bucket(tf: Timeframe, t: i64) -> (i64, i64) {
    let day = t / DAY;
    match tf {
        Timeframe::Seconds(n) => {
            let step = i64::from(n) * S;
            let start = t - t % step;
            (start, start + step)
        }
        Timeframe::Week => {
            // 1970-01-01 fue jueves: los lunes son los días ≡ 4 (mód 7).
            let mut d = day;
            while (d - 4) % 7 != 0 {
                d -= 1;
            }
            (d * DAY, (d + 7) * DAY)
        }
        Timeframe::Months(n) => {
            let (y, m, month_start) = naive_civil(day);
            let mut start = month_start;
            let mut mm = m;
            while (mm - 1) % n != 0 {
                mm -= 1;
                start -= month_len(y, mm);
            }
            let mut end = start;
            let (mut ey, mut em) = (y, mm);
            for _ in 0..n {
                end += month_len(ey, em);
                em += 1;
                if em == 13 {
                    em = 1;
                    ey += 1;
                }
            }
            (start * DAY, end * DAY)
        }
        Timeframe::Year => {
            let (y, _, _) = naive_civil(day);
            let mut start = 0;
            for yy in 1970..y {
                start += if is_leap(yy) { 366 } else { 365 };
            }
            let len = if is_leap(y) { 366 } else { 365 };
            (start * DAY, (start + len) * DAY)
        }
    }
}

/// Agregador de referencia ingenuo: agrupa todos los trades por vela y la arma de una vez.
fn naive_bars(tf: Timeframe, trades: &[TradeTick]) -> Vec<Bar> {
    let mut groups: BTreeMap<(i64, i64), Vec<(usize, &TradeTick)>> = BTreeMap::new();
    for (i, t) in trades.iter().enumerate() {
        groups
            .entry(naive_bucket(tf, t.ts_event))
            .or_default()
            .push((i, t));
    }
    groups
        .into_iter()
        .map(|((start, end), mut ts)| {
            let sum = |f: &dyn Fn(&TradeTick) -> Option<Decimal>| {
                ts.iter()
                    .filter_map(|(_, t)| f(t))
                    .fold(Decimal::ZERO, |a, b| a.checked_add(b).unwrap())
            };
            let volume_base = sum(&|t| Some(t.qty_base));
            let volume_quote = sum(&|t| Some(t.price.checked_mul(t.qty_base).unwrap()));
            let buy = sum(&|t| (t.aggressor == Side::Buy).then_some(t.qty_base));
            let sell = sum(&|t| (t.aggressor == Side::Sell).then_some(t.qty_base));
            ts.sort_by_key(|(i, t)| (t.ts_event, *i));
            Bar {
                timeframe: tf,
                start,
                end,
                open: ts[0].1.price,
                high: ts.iter().map(|(_, t)| t.price).max().unwrap(),
                low: ts.iter().map(|(_, t)| t.price).min().unwrap(),
                close: ts.last().unwrap().1.price,
                volume_base,
                volume_quote,
                buy_volume_base: buy,
                sell_volume_base: sell,
                trade_count: ts.len() as u64,
                first_ts_event: ts[0].1.ts_event,
                last_ts_event: ts.last().unwrap().1.ts_event,
                revision: 1,
                state: BarState::Closed,
            }
        })
        .collect()
}

fn tick(ts_event: i64, price_cents: i64, qty_milli: i64, side: Side) -> TradeTick {
    TradeTick {
        ts_event,
        price: Decimal::new(price_cents.into(), 2).unwrap(),
        qty_base: Decimal::new(qty_milli.into(), 3).unwrap(),
        aggressor: side,
    }
}

fn all_timeframes() -> Vec<Timeframe> {
    let mut tfs = Timeframe::STANDARD.to_vec();
    tfs.push(Timeframe::custom_seconds(90).unwrap());
    tfs.push(Timeframe::custom_seconds(7).unwrap());
    tfs
}

fn closed_by_tf(events: &[BarEvent]) -> BTreeMap<Timeframe, Vec<Bar>> {
    let mut out: BTreeMap<Timeframe, Vec<Bar>> = BTreeMap::new();
    for e in events {
        assert_eq!(e.kind, BarEventKind::Closed);
        out.entry(e.bar.timeframe).or_default().push(e.bar.clone());
    }
    out
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]

    /// Para trades aleatorios sin tardíos, las velas de cualquier temporalidad equivalen
    /// a las del agregador de referencia.
    #[test]
    fn matches_naive_aggregator(
        steps in prop::collection::vec(
            (prop_oneof![0i64..3 * S, 0i64..2 * 3_600 * S, 0i64..20 * DAY],
             1i64..10_000_000, 1i64..5_000, 0u8..3),
            1..250,
        ),
        start_day in 19_000i64..20_000,
    ) {
        let mut t = start_day * DAY;
        let trades: Vec<TradeTick> = steps
            .iter()
            .map(|&(dt, price, qty, side)| {
                t += dt;
                let side = [Side::Buy, Side::Sell, Side::Unspecified][side as usize];
                tick(t, price, qty, side)
            })
            .collect();
        let tfs = all_timeframes();
        let mut agg = BarAggregator::new(SRC, &tfs, BarConfig::default());
        let mut events = Vec::new();
        for tr in &trades {
            agg.on_trade(tr, tr.ts_event, &mut events).unwrap();
        }
        // Un latido muy posterior cierra todo lo que quedaba en formación.
        agg.on_heartbeat(t + 400 * DAY, &mut events);
        let got = closed_by_tf(&events);
        for tf in tfs {
            prop_assert_eq!(got.get(&tf).cloned().unwrap_or_default(), naive_bars(tf, &trades), "{}", tf);
        }
        prop_assert_eq!(agg.late_dropped(), 0);
    }
}

#[test]
fn does_not_close_before_end_plus_l_and_a_heartbeat_closes_it() {
    let l = BarConfig::default().lateness_ns;
    let t0 = ts(2026, 10, 27, 14, 0);
    let mut agg = BarAggregator::new(SRC, &[Timeframe::Seconds(60)], BarConfig::default());
    let mut out = Vec::new();
    agg.on_trade(&tick(t0 + 5 * S, 100, 1, Side::Buy), t0 + 5 * S, &mut out)
        .unwrap();
    // Latidos hasta fin + L − 1 ns: sigue en formación.
    for hb in [t0 + 60 * S, t0 + 60 * S + l - 1] {
        agg.on_heartbeat(hb, &mut out);
        assert!(out.is_empty(), "cerró antes de fin + L (latido en {hb})");
    }
    assert_eq!(agg.forming(Timeframe::Seconds(60)).count(), 1);
    // Latido en fin + L: cierra.
    agg.on_heartbeat(t0 + 60 * S + l, &mut out);
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].kind, BarEventKind::Closed);
    assert_eq!(out[0].ts_available, t0 + 60 * S + l);
    assert_eq!((out[0].bar.start, out[0].bar.end), (t0, t0 + 60 * S));

    // También cierra por marca de agua: un trade con ts_event = fin + L en la vela siguiente.
    let mut agg = BarAggregator::new(SRC, &[Timeframe::Seconds(60)], BarConfig::default());
    let mut out = Vec::new();
    agg.on_trade(&tick(t0, 100, 1, Side::Buy), t0, &mut out)
        .unwrap();
    agg.on_trade(
        &tick(t0 + 60 * S + l - 1, 101, 1, Side::Buy),
        t0 + 61 * S,
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    agg.on_trade(
        &tick(t0 + 60 * S + l, 102, 1, Side::Buy),
        t0 + 62 * S,
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bar.close, Decimal::new(100, 2).unwrap());
}

#[test]
fn late_trade_produces_revision_2_and_revision_1_stays() {
    let t0 = ts(2026, 10, 27, 14, 0);
    let tf = Timeframe::Seconds(60);
    let mut agg = BarAggregator::new(SRC, &[tf], BarConfig::default());
    let mut out = Vec::new();
    agg.on_trade(&tick(t0 + S, 100, 1_000, Side::Buy), t0 + S, &mut out)
        .unwrap();
    agg.on_heartbeat(t0 + 63 * S, &mut out); // cierra [t0, t0+60s)
    assert_eq!(out.len(), 1);
    let rev1 = out[0].bar.clone();
    assert_eq!((rev1.revision, rev1.state), (1, BarState::Closed));

    // Trade tardío de la vela ya cerrada (p. ej. recuperado después de un hueco).
    out.clear();
    agg.on_trade(
        &tick(t0 + 30 * S, 120, 500, Side::Sell),
        t0 + 70 * S,
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    let rev2 = &out[0];
    assert_eq!(rev2.kind, BarEventKind::Revised);
    assert_eq!(rev2.ts_available, t0 + 70 * S);
    assert_eq!(rev2.bar.revision, 2);
    assert_eq!(rev2.bar.high, Decimal::new(120, 2).unwrap());
    assert_eq!(
        rev2.bar.close,
        Decimal::new(120, 2).unwrap(),
        "el tardío es el de mayor ts_event"
    );
    assert_eq!(rev2.bar.trade_count, 2);
    assert_eq!(rev2.bar.sell_volume_base, Decimal::new(500, 3).unwrap());

    let history = agg.revisions(tf, t0).unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0], rev1, "la revisión 1 sigue disponible tal cual");
    assert_eq!(history[1], rev2.bar);
}

#[test]
fn trades_older_than_the_revision_horizon_are_counted_not_applied() {
    let t0 = ts(2026, 10, 27, 0, 0);
    let config = BarConfig {
        revision_horizon_ns: 3_600 * S,
        ..BarConfig::default()
    };
    let mut agg = BarAggregator::new(SRC, &[Timeframe::Seconds(60)], config);
    let mut out = Vec::new();
    agg.on_trade(&tick(t0, 100, 1, Side::Buy), t0, &mut out)
        .unwrap();
    agg.on_heartbeat(t0 + 3 * 3_600 * S, &mut out);
    out.clear();
    agg.on_trade(
        &tick(t0 + S, 999, 1, Side::Buy),
        t0 + 3 * 3_600 * S + 1,
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    assert_eq!(agg.late_dropped(), 1);
    assert!(
        agg.revisions(Timeframe::Seconds(60), t0).is_none(),
        "ya se descartó por horizonte"
    );
}

#[test]
fn calendar_edges() {
    let cases = [
        // Cambio de mes (enero → febrero) y febrero de año bisiesto.
        (
            Timeframe::Months(1),
            ts(2028, 1, 31, 23, 59),
            ts(2028, 1, 1, 0, 0),
            ts(2028, 2, 1, 0, 0),
        ),
        (
            Timeframe::Months(1),
            ts(2028, 2, 29, 12, 0),
            ts(2028, 2, 1, 0, 0),
            ts(2028, 3, 1, 0, 0),
        ),
        (
            Timeframe::Months(1),
            ts(2027, 2, 28, 12, 0),
            ts(2027, 2, 1, 0, 0),
            ts(2027, 3, 1, 0, 0),
        ),
        (
            Timeframe::Months(1),
            ts(2026, 12, 31, 23, 59),
            ts(2026, 12, 1, 0, 0),
            ts(2027, 1, 1, 0, 0),
        ),
        // Trimestres.
        (
            Timeframe::Months(3),
            ts(2026, 10, 27, 14, 32),
            ts(2026, 10, 1, 0, 0),
            ts(2027, 1, 1, 0, 0),
        ),
        (
            Timeframe::Months(3),
            ts(2028, 3, 31, 23, 59),
            ts(2028, 1, 1, 0, 0),
            ts(2028, 4, 1, 0, 0),
        ),
        // Año bisiesto y no bisiesto.
        (
            Timeframe::Year,
            ts(2028, 12, 31, 23, 59),
            ts(2028, 1, 1, 0, 0),
            ts(2029, 1, 1, 0, 0),
        ),
        (
            Timeframe::Year,
            ts(2026, 6, 1, 0, 0),
            ts(2026, 1, 1, 0, 0),
            ts(2027, 1, 1, 0, 0),
        ),
        // Semana que cruza el año: lunes 2024-12-30 a lunes 2025-01-06.
        (
            Timeframe::Week,
            ts(2025, 1, 1, 12, 0),
            ts(2024, 12, 30, 0, 0),
            ts(2025, 1, 6, 0, 0),
        ),
        (
            Timeframe::Week,
            ts(2024, 12, 30, 0, 0),
            ts(2024, 12, 30, 0, 0),
            ts(2025, 1, 6, 0, 0),
        ),
        (
            Timeframe::Week,
            ts(2025, 1, 5, 23, 59),
            ts(2024, 12, 30, 0, 0),
            ts(2025, 1, 6, 0, 0),
        ),
        // Día y 4h alineados al epoch.
        (
            Timeframe::Seconds(86_400),
            ts(2028, 2, 29, 23, 59),
            ts(2028, 2, 29, 0, 0),
            ts(2028, 3, 1, 0, 0),
        ),
        (
            Timeframe::Seconds(14_400),
            ts(2026, 10, 27, 14, 32),
            ts(2026, 10, 27, 12, 0),
            ts(2026, 10, 27, 16, 0),
        ),
    ];
    for (tf, t, start, end) in cases {
        let s = tf.bucket_start(t);
        assert_eq!((s, tf.bucket_end(s)), (start, end), "{tf} en {t}");
        assert_eq!(naive_bucket(tf, t), (start, end), "referencia {tf}");
    }
    // 3D: bloques de 3 días desde 1970-01-01.
    let tf = Timeframe::Seconds(259_200);
    let s = tf.bucket_start(ts(2026, 10, 27, 0, 0));
    assert_eq!(s.rem_euclid(3 * DAY), 0);
    assert_eq!(
        Timeframe::STANDARD.map(|t| t.to_string()).join(" "),
        "1s 5s 15s 1m 3m 5m 15m 30m 1h 2h 4h 6h 12h 1D 3D 1W 1M 3M 1Y"
    );
}

#[test]
fn ignores_other_payloads_and_closes_on_heartbeat_envelopes() {
    use bender_types::proto::{self, envelope};
    let t0 = ts(2026, 10, 27, 14, 0);
    let mut agg = BarAggregator::new(SRC, &[Timeframe::Seconds(1)], BarConfig::default());
    let mut out = Vec::new();
    let trade = proto::Envelope {
        ts_event: t0,
        ts_available: t0,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(proto::market_event::Event::Trade(proto::Trade {
                price: Some(Decimal::new(8_812_340, 2).unwrap().into()),
                qty_base: Some(Decimal::new(12, 3).unwrap().into()),
                aggressor_side: Side::Sell as i32,
                venue_trade_id: None,
            })),
        })),
        ..Default::default()
    };
    agg.on_envelope(&trade, &mut out).unwrap();
    let mark = proto::Envelope {
        ts_available: t0 + 10 * S,
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(proto::market_event::Event::MarkPrice(proto::MarkPrice {
                price: None,
            })),
        })),
        ..Default::default()
    };
    agg.on_envelope(&mark, &mut out).unwrap();
    assert!(out.is_empty(), "un mark price no cierra velas");
    let beat = proto::Envelope {
        ts_available: t0 + 3 * S,
        payload: Some(envelope::Payload::Heartbeat(proto::Heartbeat {})),
        ..Default::default()
    };
    agg.on_envelope(&beat, &mut out).unwrap();
    assert_eq!(out.len(), 1);
    assert_eq!(out[0].bar.sell_volume_base, Decimal::new(12, 3).unwrap());
    let bad = proto::Envelope {
        payload: Some(envelope::Payload::Market(proto::MarketEvent {
            event: Some(proto::market_event::Event::Trade(proto::Trade::default())),
        })),
        ..Default::default()
    };
    assert!(agg.on_envelope(&bad, &mut out).is_err());
}
