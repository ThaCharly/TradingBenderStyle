//! Proyección de cobertura (ADR-0011): a partir de los `Gap` y `GapRecovery` procesados,
//! `coverage(fuente, rango, as_of)` dice qué partes del rango están `complete`,
//! `recovered` o `incomplete`, tal como se sabía en `as_of`.

use std::collections::BTreeMap;

use bender_types::proto::{self, CoverageState, RecoveryResult, envelope};
use bender_types::source_key;

/// Un tramo de cobertura `[from, to)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Coverage {
    pub from: i64,
    pub to: i64,
    pub state: CoverageState,
}

#[derive(Clone, Debug)]
struct GapRecord {
    source: String,
    from: i64,
    to: i64,
    known_at: i64,
    /// Tramos recuperados `[desde, hasta)` con el instante en que se supo.
    recovered: Vec<(i64, i64, i64)>,
}

/// Proyección de huecos y recuperaciones. Se alimenta con todos los `Envelope` en orden
/// `K`; ignora lo que no es `Gap` ni `GapRecovery`.
#[derive(Clone, Debug, Default)]
pub struct CoverageProjection {
    gaps: BTreeMap<Vec<u8>, GapRecord>,
}

impl CoverageProjection {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply(&mut self, env: &proto::Envelope) {
        match &env.payload {
            Some(envelope::Payload::Gap(gap)) => {
                let source = gap
                    .source
                    .as_ref()
                    .or(env.source.as_ref())
                    .map(source_key)
                    .unwrap_or_default();
                let id = gap
                    .gap_id
                    .as_ref()
                    .map(|u| u.value.clone())
                    .unwrap_or_default();
                self.gaps.entry(id).or_insert(GapRecord {
                    source,
                    from: gap.from_ts,
                    to: gap.to_ts,
                    known_at: env.ts_available,
                    recovered: Vec::new(),
                });
            }
            Some(envelope::Payload::GapRecovery(rec)) => {
                let id = rec
                    .gap_id
                    .as_ref()
                    .map(|u| u.value.clone())
                    .unwrap_or_default();
                let Some(gap) = self.gaps.get_mut(&id) else {
                    return; // recuperación de un hueco desconocido: no cambia nada
                };
                let part = match RecoveryResult::try_from(rec.result) {
                    Ok(RecoveryResult::Full) => Some((gap.from, gap.to)),
                    Ok(RecoveryResult::Partial) => rec
                        .recovered_from
                        .zip(rec.recovered_to)
                        .map(|(a, b)| (a.max(gap.from), b.min(gap.to)))
                        .filter(|(a, b)| a < b),
                    _ => None,
                };
                if let Some((a, b)) = part {
                    gap.recovered.push((a, b, env.ts_available));
                }
            }
            _ => {}
        }
    }

    /// Cobertura de `source` en `[from, to)` según lo sabido hasta `as_of`
    /// (`ts_available ≤ as_of`). Los tramos vienen ordenados, sin huecos entre sí, y
    /// tramos vecinos tienen estados distintos.
    pub fn coverage(&self, source: &str, from: i64, to: i64, as_of: i64) -> Vec<Coverage> {
        if from >= to {
            return Vec::new();
        }
        let gaps: Vec<&GapRecord> = self
            .gaps
            .values()
            .filter(|g| g.source == source && g.known_at <= as_of && g.from < to && g.to > from)
            .collect();
        let mut points = vec![from, to];
        for g in &gaps {
            points.extend([g.from.max(from), g.to.min(to)]);
            for &(a, b, known) in &g.recovered {
                if known <= as_of {
                    points.extend([a.clamp(from, to), b.clamp(from, to)]);
                }
            }
        }
        points.sort_unstable();
        points.dedup();

        let mut out: Vec<Coverage> = Vec::new();
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            let covering = gaps.iter().filter(|g| g.from <= a && g.to >= b);
            let mut state = CoverageState::Complete;
            for g in covering {
                let recovered = g
                    .recovered
                    .iter()
                    .any(|&(ra, rb, known)| known <= as_of && ra <= a && rb >= b);
                if !recovered {
                    state = CoverageState::Incomplete;
                    break;
                }
                state = CoverageState::Recovered;
            }
            match out.last_mut() {
                Some(last) if last.state == state && last.to == a => last.to = b,
                _ => out.push(Coverage {
                    from: a,
                    to: b,
                    state,
                }),
            }
        }
        out
    }
}
