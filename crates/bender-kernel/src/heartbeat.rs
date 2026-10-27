//! Latidos sintéticos para fuentes reconstruidas (doc 05 §3): uno por punto de la grilla
//! `G` alineada al epoch, con `provenance = synthetic`.

use bender_types::proto::{self, envelope, source_id};

/// Grilla por defecto: 100 ms.
pub const DEFAULT_GRID_NS: i64 = 100_000_000;

fn clock_source() -> proto::SourceId {
    proto::SourceId {
        kind: Some(source_id::Kind::Clock(proto::ClockSource {})),
    }
}

/// Latidos en los puntos de la grilla `grid_ns` dentro de `[from, to)`. Los instantes son
/// múltiplos de `grid_ns` (alineados al epoch), así que la cantidad es
/// `ceil(to/G) − ceil(from/G)`; si `from` está en la grilla, `ceil((to − from)/G)`.
///
/// El `ingest_seq` de cada latido es su índice en la grilla (`ts / G`): partir un rango en
/// tramos produce exactamente los mismos latidos que generarlo de una vez.
pub fn synthetic_heartbeats(
    from: i64,
    to: i64,
    grid_ns: i64,
) -> impl Iterator<Item = proto::Envelope> {
    assert!(grid_ns > 0, "la grilla tiene que ser positiva");
    let first = ceil_div(from, grid_ns);
    let end = ceil_div(to, grid_ns);
    (first..end).map(move |index| {
        let ts = index * grid_ns;
        proto::Envelope {
            ts_event: ts,
            ts_received: None,
            ts_available: ts,
            source: Some(clock_source()),
            source_seq: None,
            ingest_seq: index as u64,
            provenance: proto::Provenance::Synthetic as i32,
            payload: Some(envelope::Payload::Heartbeat(proto::Heartbeat {})),
        }
    })
}

fn ceil_div(a: i64, b: i64) -> i64 {
    let q = a.div_euclid(b);
    if a.rem_euclid(b) == 0 { q } else { q + 1 }
}
