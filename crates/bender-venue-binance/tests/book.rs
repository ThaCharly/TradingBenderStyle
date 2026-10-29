//! Libro local: reglas de sincronización de spot y USDⓈ-M, huecos y top-N.

use bender_types::Decimal;
use bender_types::proto::{self, BookSide};
use bender_venue_binance::Market;
use bender_venue_binance::book::{DiffOutcome, LocalBook, SnapshotOutcome};
use bender_venue_binance::normalize::{DepthSnapshot, DepthUpdate};

fn lv(p: &str, q: &str) -> [String; 2] {
    [p.to_string(), q.to_string()]
}

fn diff(
    first: u64,
    last: u64,
    pu: Option<u64>,
    bids: &[(&str, &str)],
    asks: &[(&str, &str)],
) -> DepthUpdate {
    DepthUpdate {
        event_time: 0,
        symbol: "BTCUSDT".into(),
        first_update_id: first,
        final_update_id: last,
        prev_final_update_id: pu,
        bids: bids.iter().map(|(p, q)| lv(p, q)).collect(),
        asks: asks.iter().map(|(p, q)| lv(p, q)).collect(),
    }
}

fn snapshot(id: u64, bids: &[(&str, &str)], asks: &[(&str, &str)]) -> DepthSnapshot {
    DepthSnapshot {
        last_update_id: id,
        event_time: None,
        bids: bids.iter().map(|(p, q)| lv(p, q)).collect(),
        asks: asks.iter().map(|(p, q)| lv(p, q)).collect(),
    }
}

fn d(s: &str) -> Decimal {
    s.parse().unwrap()
}

fn synced(o: SnapshotOutcome) -> (proto::BookSnapshot, Vec<proto::BookDelta>) {
    match o {
        SnapshotOutcome::Synced { snapshot, deltas } => {
            (snapshot, deltas.into_iter().map(|(_, d)| d).collect())
        }
        other => panic!("esperaba sincronizado: {other:?}"),
    }
}

#[test]
fn spot_sync_rules() {
    let mut book = LocalBook::new(Market::Spot, 20);
    // Antes del snapshot: se guardan.
    assert_eq!(
        book.on_diff(&diff(95, 100, None, &[("10", "1")], &[]))
            .unwrap(),
        DiffOutcome::Buffered
    );
    assert_eq!(
        book.on_diff(&diff(101, 103, None, &[("11", "1")], &[]))
            .unwrap(),
        DiffOutcome::Buffered
    );
    assert_eq!(
        book.on_diff(&diff(104, 104, None, &[("12", "1")], &[]))
            .unwrap(),
        DiffOutcome::Buffered
    );
    // Snapshot 102: el delta [95,100] es viejo; [101,103] engancha (U ≤ 103 ≤ u).
    let (snap, deltas) = synced(
        book.on_snapshot(&snapshot(102, &[("9", "5")], &[("20", "1")]))
            .unwrap(),
    );
    assert_eq!(snap.venue_seq, Some(102));
    assert_eq!(deltas.len(), 2);
    assert_eq!(deltas[0].venue_seq, Some(103));
    assert_eq!(book.last_update_id(), Some(104));
    let (bids, _) = book.top();
    assert_eq!(
        bids,
        vec![(d("12"), d("1")), (d("11"), d("1")), (d("9"), d("5"))]
    );
    // Viejo → Stale; continuo → aplicado; salto → Gap.
    assert_eq!(
        book.on_diff(&diff(103, 104, None, &[], &[])).unwrap(),
        DiffOutcome::Stale
    );
    assert!(matches!(
        book.on_diff(&diff(105, 106, None, &[], &[("21", "1")]))
            .unwrap(),
        DiffOutcome::Applied(Some(_))
    ));
    assert_eq!(
        book.on_diff(&diff(108, 109, None, &[], &[])).unwrap(),
        DiffOutcome::Gap {
            expected: 107,
            got: 108
        }
    );
    assert!(!book.is_synced());
}

#[test]
fn usdm_sync_rules() {
    let mut book = LocalBook::new(Market::UsdM, 20);
    book.on_diff(&diff(90, 99, Some(89), &[("1", "1")], &[]))
        .unwrap();
    book.on_diff(&diff(100, 105, Some(99), &[("2", "1")], &[]))
        .unwrap();
    book.on_diff(&diff(106, 110, Some(105), &[("3", "1")], &[]))
        .unwrap();
    // Snapshot 103: se descarta u < 103; el primero cumple U ≤ 103 ≤ u.
    let (_, deltas) = synced(book.on_snapshot(&snapshot(103, &[], &[])).unwrap());
    assert_eq!(
        deltas.iter().map(|d| d.venue_seq).collect::<Vec<_>>(),
        vec![Some(105), Some(110)]
    );
    // Después, pu tiene que ser el u anterior.
    assert!(matches!(
        book.on_diff(&diff(111, 112, Some(110), &[("4", "1")], &[]))
            .unwrap(),
        DiffOutcome::Applied(_)
    ));
    assert_eq!(
        book.on_diff(&diff(114, 115, Some(113), &[], &[])).unwrap(),
        DiffOutcome::Gap {
            expected: 112,
            got: 113
        }
    );
}

#[test]
fn usdm_first_diff_may_continue_exactly_from_the_snapshot() {
    let mut book = LocalBook::new(Market::UsdM, 20);
    book.on_diff(&diff(101, 102, Some(100), &[("1", "1")], &[]))
        .unwrap();
    let (_, deltas) = synced(book.on_snapshot(&snapshot(100, &[], &[])).unwrap());
    assert_eq!(deltas.len(), 1);
    // Sin continuidad: hueco.
    let mut book = LocalBook::new(Market::UsdM, 20);
    book.on_diff(&diff(101, 102, Some(99), &[("1", "1")], &[]))
        .unwrap();
    assert!(matches!(
        book.on_snapshot(&snapshot(100, &[], &[])).unwrap(),
        SnapshotOutcome::Retry { .. }
    ));
}

#[test]
fn old_snapshot_keeps_buffer_and_retries() {
    let mut book = LocalBook::new(Market::Spot, 20);
    book.on_diff(&diff(200, 210, None, &[("1", "1")], &[]))
        .unwrap();
    book.on_diff(&diff(211, 215, None, &[("2", "1")], &[]))
        .unwrap();
    // Snapshot demasiado viejo: el primer delta guardado no engancha.
    assert_eq!(
        book.on_snapshot(&snapshot(150, &[], &[])).unwrap(),
        SnapshotOutcome::Retry {
            expected: 151,
            got: 200
        }
    );
    assert!(!book.is_synced());
    // Los deltas siguen guardados: un snapshot nuevo engancha con ellos.
    let (_, deltas) = synced(book.on_snapshot(&snapshot(212, &[], &[])).unwrap());
    assert_eq!(deltas.len(), 1);
    assert_eq!(book.last_update_id(), Some(215));
}

#[test]
fn gap_keeps_the_diff_for_the_next_snapshot() {
    let mut book = LocalBook::new(Market::Spot, 20);
    synced(book.on_snapshot(&snapshot(10, &[], &[])).unwrap());
    assert!(matches!(
        book.on_diff(&diff(20, 25, None, &[("5", "1")], &[]))
            .unwrap(),
        DiffOutcome::Gap { .. }
    ));
    // El snapshot 22 cae dentro del delta que no enganchó: ese delta se aplica.
    let (_, deltas) = synced(book.on_snapshot(&snapshot(22, &[], &[])).unwrap());
    assert_eq!(deltas.len(), 1);
    assert_eq!(book.top().0, vec![(d("5"), d("1"))]);
}

#[test]
fn deltas_only_report_top_n_changes() {
    let mut book = LocalBook::new(Market::Spot, 2);
    synced(
        book.on_snapshot(&snapshot(1, &[("10", "1"), ("9", "1"), ("8", "1")], &[]))
            .unwrap(),
    );
    // Cambio fuera del top-2: no hay delta.
    assert_eq!(
        book.on_diff(&diff(2, 2, None, &[("8", "3")], &[])).unwrap(),
        DiffOutcome::Applied(None)
    );
    // Entra un nivel mejor: aparece 11 y sale 9 (cantidad 0).
    let DiffOutcome::Applied(Some(delta)) = book
        .on_diff(&diff(3, 3, None, &[("11", "2")], &[]))
        .unwrap()
    else {
        panic!("esperaba delta");
    };
    let changes: Vec<(i32, Decimal, Decimal)> = delta
        .changes
        .iter()
        .map(|c| {
            (
                c.side,
                c.price.unwrap().try_into().unwrap(),
                c.qty_base.unwrap().try_into().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        changes,
        vec![
            (BookSide::Bid as i32, d("11"), d("2")),
            (BookSide::Bid as i32, d("9"), Decimal::ZERO)
        ]
    );
    // Se va el mejor: vuelve a entrar 9.
    let DiffOutcome::Applied(Some(delta)) = book
        .on_diff(&diff(4, 4, None, &[("11", "0")], &[]))
        .unwrap()
    else {
        panic!("esperaba delta");
    };
    assert_eq!(delta.changes.len(), 2);
}

#[test]
fn invalidate_drops_the_buffer() {
    let mut book = LocalBook::new(Market::Spot, 20);
    book.on_diff(&diff(5, 6, None, &[("1", "1")], &[])).unwrap();
    book.invalidate();
    let (_, deltas) = synced(book.on_snapshot(&snapshot(4, &[], &[])).unwrap());
    assert!(deltas.is_empty());
}
