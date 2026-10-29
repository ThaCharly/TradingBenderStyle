//! Libro local de Binance: snapshot REST + deltas del WS, con las reglas de
//! sincronización de cada mercado, y salida canónica restringida al top-N.
//!
//! - **Spot**: después del snapshot `L`, se descartan los deltas con `u ≤ L`; el primero
//!   aplicado cumple `U ≤ L+1 ≤ u`; cada siguiente, `U = u_anterior + 1`.
//! - **USDⓈ-M**: se descartan los deltas con `u < L`; el primero aplicado cumple
//!   `U ≤ L ≤ u` (regla de la documentación) o `pu = L` (continúa exactamente desde el
//!   snapshot); cada siguiente, `pu = u_anterior`.
//!
//! Si la secuencia se rompe, el libro deja de ser válido ([`DiffOutcome::Gap`]) hasta un
//! snapshot nuevo. Mientras no hay snapshot, los deltas se guardan para aplicarlos
//! después.

use std::collections::BTreeMap;

use bender_types::Decimal;
use bender_types::proto::{self, BookSide};

use crate::Market;
use crate::normalize::{DepthSnapshot, DepthUpdate, NormalizeError};

/// Qué pasó con un delta.
#[derive(Clone, Debug, PartialEq)]
pub enum DiffOutcome {
    /// Aplicado. Trae el delta canónico del top-N si algo cambió en él.
    Applied(Option<proto::BookDelta>),
    /// Viejo respecto del snapshot: se descarta.
    Stale,
    /// Guardado hasta que llegue el snapshot.
    Buffered,
    /// Secuencia rota: hace falta un snapshot nuevo.
    Gap { expected: u64, got: u64 },
}

/// Resultado de cargar un snapshot.
#[derive(Clone, Debug, PartialEq)]
pub enum SnapshotOutcome {
    /// Libro sincronizado: snapshot canónico del top-N y los deltas guardados aplicados
    /// después, cada uno con el `E` (ms) de su mensaje.
    Synced {
        snapshot: proto::BookSnapshot,
        deltas: Vec<(i64, proto::BookDelta)>,
    },
    /// Los deltas guardados no enganchan: hace falta otro snapshot.
    Retry { expected: u64, got: u64 },
}

/// Deltas guardados como máximo mientras no hay snapshot.
const MAX_BUFFER: usize = 10_000;

/// Niveles `(precio, cantidad)` de un lado.
pub type Levels = Vec<(Decimal, Decimal)>;

/// Libro local de un símbolo.
#[derive(Clone, Debug)]
pub struct LocalBook {
    market: Market,
    top_n: usize,
    bids: BTreeMap<Decimal, Decimal>,
    asks: BTreeMap<Decimal, Decimal>,
    /// `lastUpdateId` del snapshot vigente; `None` = sin snapshot.
    snapshot_id: Option<u64>,
    /// `u` del último delta aplicado.
    last_u: Option<u64>,
    buffer: Vec<DepthUpdate>,
}

fn parse_levels(levels: &[[String; 2]]) -> Result<Vec<(Decimal, Decimal)>, NormalizeError> {
    levels
        .iter()
        .map(|[p, q]| Ok((p.parse()?, q.parse()?)))
        .collect::<Result<_, bender_types::DecimalError>>()
        .map_err(NormalizeError::from)
}

impl LocalBook {
    pub fn new(market: Market, top_n: usize) -> Self {
        Self {
            market,
            top_n,
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            snapshot_id: None,
            last_u: None,
            buffer: Vec::new(),
        }
    }

    pub fn is_synced(&self) -> bool {
        self.snapshot_id.is_some()
    }

    /// `u` del último delta aplicado (o el `lastUpdateId` del snapshot).
    pub fn last_update_id(&self) -> Option<u64> {
        self.last_u.or(self.snapshot_id)
    }

    /// Mejores `top_n` niveles de cada lado: compras de mayor a menor, ventas de menor a
    /// mayor.
    pub fn top(&self) -> (Levels, Levels) {
        let bids = self
            .bids
            .iter()
            .rev()
            .take(self.top_n)
            .map(|(p, q)| (*p, *q))
            .collect();
        let asks = self
            .asks
            .iter()
            .take(self.top_n)
            .map(|(p, q)| (*p, *q))
            .collect();
        (bids, asks)
    }

    fn snapshot_proto(&self, venue_seq: u64) -> proto::BookSnapshot {
        let (bids, asks) = self.top();
        let level = |(p, q): (Decimal, Decimal)| proto::BookLevel {
            price: Some(p.into()),
            qty_base: Some(q.into()),
        };
        proto::BookSnapshot {
            bids: bids.into_iter().map(level).collect(),
            asks: asks.into_iter().map(level).collect(),
            venue_seq: Some(venue_seq),
        }
    }

    /// Invalida el libro (después de un hueco o una desconexión) y descarta los deltas
    /// guardados: hasta un snapshot nuevo, los deltas que lleguen se guardan.
    pub fn invalidate(&mut self) {
        self.buffer.clear();
        self.snapshot_id = None;
        self.last_u = None;
        self.bids.clear();
        self.asks.clear();
    }

    /// Carga un snapshot REST y aplica los deltas guardados que correspondan.
    ///
    /// Si los deltas guardados no enganchan con el snapshot (el snapshot es viejo o hubo
    /// un salto entre ellos), el libro sigue sin sincronizar, los deltas desde el que no
    /// enganchó quedan guardados y hace falta otro snapshot ([`SnapshotOutcome::Retry`]).
    pub fn on_snapshot(
        &mut self,
        snapshot: &DepthSnapshot,
    ) -> Result<SnapshotOutcome, NormalizeError> {
        self.bids = parse_levels(&snapshot.bids)?
            .into_iter()
            .filter(|(_, q)| !q.is_zero())
            .collect();
        self.asks = parse_levels(&snapshot.asks)?
            .into_iter()
            .filter(|(_, q)| !q.is_zero())
            .collect();
        self.snapshot_id = Some(snapshot.last_update_id);
        self.last_u = None;
        let snap = self.snapshot_proto(snapshot.last_update_id);
        let mut deltas = Vec::new();
        let mut buffered = std::mem::take(&mut self.buffer).into_iter();
        while let Some(d) = buffered.next() {
            match self.on_diff(&d)? {
                DiffOutcome::Applied(Some(delta)) => deltas.push((d.event_time, delta)),
                DiffOutcome::Applied(None) | DiffOutcome::Stale | DiffOutcome::Buffered => {}
                DiffOutcome::Gap { expected, got } => {
                    self.buffer.extend(buffered);
                    return Ok(SnapshotOutcome::Retry { expected, got });
                }
            }
        }
        Ok(SnapshotOutcome::Synced {
            snapshot: snap,
            deltas,
        })
    }

    /// Aplica un delta del WS.
    pub fn on_diff(&mut self, d: &DepthUpdate) -> Result<DiffOutcome, NormalizeError> {
        let Some(snapshot_id) = self.snapshot_id else {
            if self.buffer.len() >= MAX_BUFFER {
                // Sin snapshot por mucho tiempo: lo más viejo ya no sirve (el próximo
                // snapshot va a ser más nuevo).
                self.buffer.remove(0);
            }
            self.buffer.push(d.clone());
            return Ok(DiffOutcome::Buffered);
        };
        match (self.market, self.last_u) {
            (Market::Spot, None) => {
                if d.final_update_id <= snapshot_id {
                    return Ok(DiffOutcome::Stale);
                }
                if d.first_update_id > snapshot_id + 1 {
                    return Ok(self.gap(d, snapshot_id + 1, d.first_update_id));
                }
            }
            (Market::Spot, Some(last)) => {
                if d.final_update_id <= last {
                    return Ok(DiffOutcome::Stale);
                }
                if d.first_update_id != last + 1 {
                    return Ok(self.gap(d, last + 1, d.first_update_id));
                }
            }
            (Market::UsdM, None) => {
                if d.final_update_id < snapshot_id {
                    return Ok(DiffOutcome::Stale);
                }
                // Engancha si contiene al snapshot o si continúa exactamente desde él.
                if d.first_update_id > snapshot_id && d.prev_final_update_id != Some(snapshot_id) {
                    return Ok(self.gap(d, snapshot_id, d.first_update_id));
                }
            }
            (Market::UsdM, Some(last)) => {
                if d.final_update_id <= last {
                    return Ok(DiffOutcome::Stale);
                }
                let pu = d.prev_final_update_id.unwrap_or(0);
                if pu != last {
                    return Ok(self.gap(d, last, pu));
                }
            }
        }
        let before = self.top();
        for (p, q) in parse_levels(&d.bids)? {
            if q.is_zero() {
                self.bids.remove(&p);
            } else {
                self.bids.insert(p, q);
            }
        }
        for (p, q) in parse_levels(&d.asks)? {
            if q.is_zero() {
                self.asks.remove(&p);
            } else {
                self.asks.insert(p, q);
            }
        }
        self.last_u = Some(d.final_update_id);
        let after = self.top();
        let mut changes = Vec::new();
        diff_side(BookSide::Bid, &before.0, &after.0, &mut changes);
        diff_side(BookSide::Ask, &before.1, &after.1, &mut changes);
        Ok(DiffOutcome::Applied((!changes.is_empty()).then_some(
            proto::BookDelta {
                changes,
                venue_seq: Some(d.final_update_id),
            },
        )))
    }

    /// Hueco: el libro se invalida y el delta que no enganchó queda guardado para el
    /// próximo snapshot.
    fn gap(&mut self, d: &DepthUpdate, expected: u64, got: u64) -> DiffOutcome {
        self.invalidate();
        self.buffer.push(d.clone());
        DiffOutcome::Gap { expected, got }
    }

    /// Snapshot canónico del top-N actual (para grabar periódicamente).
    pub fn snapshot(&self) -> Option<proto::BookSnapshot> {
        self.last_update_id().map(|id| self.snapshot_proto(id))
    }
}

/// Cambios del top-N de un lado: niveles nuevos o con otra cantidad, y niveles que
/// salieron del top-N (cantidad 0).
fn diff_side(
    side: BookSide,
    before: &[(Decimal, Decimal)],
    after: &[(Decimal, Decimal)],
    out: &mut Vec<proto::BookChange>,
) {
    let old: BTreeMap<Decimal, Decimal> = before.iter().copied().collect();
    let new: BTreeMap<Decimal, Decimal> = after.iter().copied().collect();
    let change = |p: Decimal, q: Decimal| proto::BookChange {
        side: side as i32,
        price: Some(p.into()),
        qty_base: Some(q.into()),
    };
    for (p, q) in &new {
        if old.get(p) != Some(q) {
            out.push(change(*p, *q));
        }
    }
    for p in old.keys() {
        if !new.contains_key(p) {
            out.push(change(*p, Decimal::ZERO));
        }
    }
}
