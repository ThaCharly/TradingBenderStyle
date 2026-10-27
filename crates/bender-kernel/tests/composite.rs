//! Criterios de #26: velas compuestas multi-venue.

use bender_kernel::{
    BarAggregator, BarConfig, BarEventKind, BarState, CompositeAggregator, CompositeConfig,
    CompositeDefinition, CompositeEvent, PriceRule, SpecHistory, Timeframe, TradeTick,
};
use bender_types::proto::{self, Side};
use bender_types::{Decimal, days_from_civil};

const S: i64 = 1_000_000_000;
const BINANCE: &str = "BINANCE:PERP_LINEAR:BTCUSDT:trades";
const BYBIT: &str = "BYBIT:PERP_LINEAR:BTCUSDT:trades";
const BLOFIN: &str = "BLOFIN:PERP_LINEAR:BTC-USDT:trades";

fn t0() -> i64 {
    days_from_civil(2026, 10, 27) * 86_400 * S + 14 * 3_600 * S
}

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn tick(ts_event: i64, price: &str, qty: &str, side: Side) -> TradeTick {
    TradeTick {
        ts_event,
        price: d(price),
        qty_base: d(qty),
        aggressor: side,
    }
}

fn def(members: &[&str], rule: PriceRule) -> CompositeDefinition {
    CompositeDefinition {
        id: "BTC/USD perps agregado".into(),
        version: 3,
        members: members.iter().map(|m| (*m).to_string()).collect(),
        price_rule: rule,
    }
}

#[test]
fn single_venue_composite_equals_that_venue_bar() {
    let tfs = [Timeframe::Seconds(60), Timeframe::Seconds(300)];
    let mut simple = BarAggregator::new(BINANCE, &tfs, BarConfig::default());
    let mut composite = CompositeAggregator::new(
        def(&[BINANCE], PriceRule::Vwap),
        &tfs,
        CompositeConfig::default(),
    );
    let (mut a, mut b) = (Vec::new(), Vec::new());
    let mut rng = 7u64;
    for i in 0..2_000i64 {
        rng = rng.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        let price = format!("{}.{:02}", 65_000 + (rng >> 40) % 300, (rng >> 20) % 100);
        let qty = format!("0.{:03}", (rng >> 8) % 999 + 1);
        let side = if rng.is_multiple_of(3) {
            Side::Sell
        } else {
            Side::Buy
        };
        let tr = tick(t0() + i * 700_000_000, &price, &qty, side);
        simple.on_trade(&tr, tr.ts_event, &mut a).unwrap();
        composite
            .on_trade(BINANCE, &tr, tr.ts_event, &mut b)
            .unwrap();
    }
    simple.on_heartbeat(t0() + 10_000 * S, &mut a);
    composite.on_heartbeat(t0() + 10_000 * S, &mut b);
    assert_eq!(a.len(), b.len());
    assert!(a.len() > 20);
    for (x, y) in a.iter().zip(&b) {
        let (s, c) = (&x.bar, &y.bar);
        let p = c.prices.as_ref().unwrap();
        assert_eq!((c.timeframe, c.start, c.end), (s.timeframe, s.start, s.end));
        assert_eq!(
            (p.open, p.high, p.low, p.close),
            (s.open, s.high, s.low, s.close)
        );
        assert_eq!(
            (p.first_ts_event, p.last_ts_event),
            (s.first_ts_event, s.last_ts_event)
        );
        assert_eq!(
            (
                c.volume_base,
                c.volume_quote,
                c.buy_volume_base,
                c.sell_volume_base,
                c.trade_count
            ),
            (
                s.volume_base,
                s.volume_quote,
                s.buy_volume_base,
                s.sell_volume_base,
                s.trade_count
            )
        );
        assert_eq!(c.per_source[BINANCE].volume_base, s.volume_base);
        // Las que cerró la marca de agua de la fuente no tienen fuentes incompletas; las
        // últimas las cierra el latido final con la fuente sin haber superado el fin.
        if y.ts_available < t0() + 10_000 * S {
            assert!(c.incomplete_sources.is_empty());
        } else {
            assert_eq!(c.incomplete_sources, vec![BINANCE.to_string()]);
        }
        assert_eq!((c.revision, c.state), (s.revision, s.state));
    }
}

#[test]
fn silent_venue_closes_after_m_marked_incomplete_and_late_data_revises() {
    let tf = Timeframe::Seconds(60);
    let cfg = CompositeConfig::default(); // L = 2 s, M = 5 s
    let mut agg = CompositeAggregator::new(def(&[BINANCE, BYBIT], PriceRule::Vwap), &[tf], cfg);
    let mut out: Vec<CompositeEvent> = Vec::new();
    // Las dos operan en la vela [t0, t0+60s); después Bybit se calla.
    agg.on_trade(
        BINANCE,
        &tick(t0() + S, "100", "1", Side::Buy),
        t0() + S,
        &mut out,
    )
    .unwrap();
    agg.on_trade(
        BYBIT,
        &tick(t0() + 2 * S, "102", "3", Side::Sell),
        t0() + 2 * S,
        &mut out,
    )
    .unwrap();
    // Binance sigue y su marca de agua supera el fin; Bybit no: todavía no cierra.
    for k in 0..4 {
        let t = t0() + 62 * S + k * S;
        agg.on_trade(BINANCE, &tick(t, "101", "0.5", Side::Buy), t, &mut out)
            .unwrap();
    }
    assert!(out.is_empty(), "esperando a Bybit");
    // Un latido en fin + L + M − 1 ns tampoco cierra; en fin + L + M sí.
    let end = t0() + 60 * S;
    agg.on_heartbeat(end + 7 * S - 1, &mut out);
    assert!(out.is_empty());
    agg.on_heartbeat(end + 7 * S, &mut out);
    assert_eq!(out.len(), 1);
    let closed = out[0].clone();
    assert_eq!(closed.kind, BarEventKind::Closed);
    assert_eq!(closed.bar.incomplete_sources, vec![BYBIT.to_string()]);
    assert_eq!(
        (
            closed.bar.definition_id.as_str(),
            closed.bar.definition_version
        ),
        ("BTC/USD perps agregado", 3)
    );
    // VWAP = (100·1 + 102·3) / 4 = 101,5.
    assert_eq!(closed.bar.vwap, Some(d("101.5")));
    assert_eq!(closed.bar.per_source[BYBIT].sell_volume_base, d("3"));

    // Bybit vuelve con un trade tardío de esa vela y luego se pone al día.
    out.clear();
    agg.on_trade(
        BYBIT,
        &tick(t0() + 50 * S, "104", "1", Side::Buy),
        end + 20 * S,
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 1);
    let rev = &out[0];
    assert_eq!(rev.kind, BarEventKind::Revised);
    assert_eq!(rev.bar.revision, 2);
    assert_eq!(rev.bar.per_source[BYBIT].trade_count, 2);
    assert_eq!(
        rev.bar.incomplete_sources,
        vec![BYBIT.to_string()],
        "Bybit todavía no superó el fin"
    );
    let t_late = end + 3 * S;
    agg.on_trade(
        BYBIT,
        &tick(end - S / 2, "103", "1", Side::Buy),
        t_late + 20 * S,
        &mut out,
    )
    .unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(out[1].bar.revision, 3);
    let history = agg.revisions(tf, t0()).unwrap();
    assert_eq!(history.len(), 3);
    assert_eq!(history[0], closed.bar, "la revisión 1 queda intacta");
}

#[test]
fn reference_price_rule_uses_only_the_reference_for_ohlc() {
    let tf = Timeframe::Seconds(60);
    let mut agg = CompositeAggregator::new(
        def(&[BINANCE, BLOFIN], PriceRule::Reference(BINANCE.into())),
        &[tf],
        CompositeConfig::default(),
    );
    let mut out = Vec::new();
    agg.on_trade(
        BLOFIN,
        &tick(t0() + S, "90", "5", Side::Buy),
        t0() + S,
        &mut out,
    )
    .unwrap();
    agg.on_trade(
        BINANCE,
        &tick(t0() + 2 * S, "100", "1", Side::Buy),
        t0() + 2 * S,
        &mut out,
    )
    .unwrap();
    agg.on_trade(
        BLOFIN,
        &tick(t0() + 3 * S, "200", "1", Side::Buy),
        t0() + 3 * S,
        &mut out,
    )
    .unwrap();
    agg.on_heartbeat(t0() + 70 * S, &mut out);
    let bar = &out[0].bar;
    let p = bar.prices.as_ref().unwrap();
    assert_eq!(
        (p.open, p.high, p.low, p.close),
        (d("100"), d("100"), d("100"), d("100"))
    );
    assert_eq!(
        bar.volume_base,
        d("7"),
        "el volumen es de todas las fuentes"
    );
    // Trades de fuentes que no son miembros: ignorados.
    out.clear();
    agg.on_trade(
        BYBIT,
        &tick(t0() + 80 * S, "1", "1000", Side::Buy),
        t0() + 80 * S,
        &mut out,
    )
    .unwrap();
    assert!(agg.forming(tf).next().is_none());
}

fn spec(
    valid_from: i64,
    valid_to: Option<i64>,
    contract_size: &str,
    unit: proto::ContractUnit,
) -> proto::InstrumentSpec {
    proto::InstrumentSpec {
        valid_from,
        valid_to,
        derivative: Some(proto::DerivativeSpec {
            contract_kind: proto::ContractKind::Linear as i32,
            contract_size: Some(d(contract_size).into()),
            contract_unit: unit as i32,
            ..Default::default()
        }),
        ..Default::default()
    }
}

#[test]
fn contracts_convert_with_the_spec_in_force_at_each_ts_event() {
    // El venue cambia el tamaño de contrato de 0.001 BTC a 0.01 BTC a mitad de la vela.
    let change = t0() + 30 * S;
    let specs = SpecHistory::new(vec![
        spec(change, None, "0.01", proto::ContractUnit::Base),
        spec(0, Some(change), "0.001", proto::ContractUnit::Base),
    ]);
    let tf = Timeframe::Seconds(60);
    let mut agg = CompositeAggregator::new(
        def(&[BLOFIN], PriceRule::Vwap),
        &[tf],
        CompositeConfig::default(),
    );
    let mut out = Vec::new();
    // (ts_event, contratos): 5 contratos antes del cambio = 0.005 BTC; 5 después = 0.05 BTC.
    for (ts, contracts) in [
        (t0() + 10 * S, "5"),
        (change - 1, "2"),
        (change, "5"),
        (t0() + 50 * S, "1"),
    ] {
        let price = d("65000");
        let qty_base = specs.to_base(ts, d(contracts), price).unwrap();
        agg.on_trade(
            BLOFIN,
            &TradeTick {
                ts_event: ts,
                price,
                qty_base,
                aggressor: Side::Buy,
            },
            ts,
            &mut out,
        )
        .unwrap();
    }
    agg.on_heartbeat(t0() + 70 * S, &mut out);
    // 5·0.001 + 2·0.001 + 5·0.01 + 1·0.01 = 0.067 BTC.
    assert_eq!(out[0].bar.volume_base, d("0.067"));
    assert!(
        specs.to_base(-1, d("1"), d("1")).is_err(),
        "antes de la primera spec"
    );

    // Inversos: contratos de 100 USD → 100 / precio BTC, con 18 decimales.
    let inverse = SpecHistory::new(vec![spec(0, None, "100", proto::ContractUnit::Quote)]);
    assert_eq!(
        inverse.to_base(t0(), d("3"), d("60000")).unwrap(),
        d("0.005")
    );
    // Spot (sin datos de derivado): la cantidad no cambia.
    let spot = SpecHistory::new(vec![proto::InstrumentSpec {
        valid_from: 0,
        ..Default::default()
    }]);
    assert_eq!(spot.to_base(t0(), d("1.5"), d("1")).unwrap(), d("1.5"));
    // Sin spec vigente: error.
    let expired = SpecHistory::new(vec![spec(0, Some(10), "1", proto::ContractUnit::Base)]);
    assert!(expired.to_base(10, d("1"), d("1")).is_err());
}

#[test]
fn closed_bar_state_and_all_sources_ready_close_without_waiting() {
    let tf = Timeframe::Seconds(60);
    let mut agg = CompositeAggregator::new(
        def(&[BINANCE, BYBIT], PriceRule::Vwap),
        &[tf],
        CompositeConfig::default(),
    );
    let mut out = Vec::new();
    agg.on_trade(
        BINANCE,
        &tick(t0() + S, "1", "1", Side::Buy),
        t0() + S,
        &mut out,
    )
    .unwrap();
    agg.on_trade(
        BYBIT,
        &tick(t0() + 62 * S, "1", "1", Side::Buy),
        t0() + 62 * S,
        &mut out,
    )
    .unwrap();
    assert!(out.is_empty());
    agg.on_trade(
        BINANCE,
        &tick(t0() + 62 * S, "1", "1", Side::Buy),
        t0() + 62 * S,
        &mut out,
    )
    .unwrap();
    assert_eq!(
        out.len(),
        1,
        "las dos superaron el fin: cierra sin esperar M"
    );
    assert_eq!(out[0].bar.state, BarState::Closed);
    assert!(out[0].bar.incomplete_sources.is_empty());
}
