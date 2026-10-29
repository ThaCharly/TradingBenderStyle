//! Reporte de una grabación: eventos por fuente, huecos, recuperaciones y tamaño en disco
//! por tipo de dato (crudo/normalizado × trades/L2/otros), para revisar ADR-0009.

use std::collections::BTreeMap;

use bender_store::{Store, StoreError, Zone};
use bender_types::proto::{GapReason, RecoveryResult, envelope};

/// Tipo de dato para el presupuesto de disco.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Category {
    Trades,
    L2,
    Other,
}

impl Category {
    /// Categoría de una clave de fuente (normalizada o cruda).
    pub fn of(source: &str) -> Self {
        if source.ends_with(":trades")
            || source.ends_with(":aggTrade")
            || source.ends_with(":aggTrades")
        {
            Self::Trades
        } else if source.ends_with(":book") || source.ends_with(":depth") {
            Self::L2
        } else {
            Self::Other
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Trades => "trades",
            Self::L2 => "L2",
            Self::Other => "otros",
        }
    }
}

/// Totales de una fuente.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SourceStats {
    pub rows: u64,
    pub bytes: u64,
    pub gaps: u64,
    pub recoveries: BTreeMap<String, u64>,
}

/// El reporte.
#[derive(Clone, Debug, Default)]
pub struct Report {
    pub sources: BTreeMap<(Zone, String), SourceStats>,
    /// Huecos por motivo.
    pub gap_reasons: BTreeMap<String, u64>,
    /// Instantes del primer y último latido.
    pub span: Option<(i64, i64)>,
}

fn reason_name(v: i32) -> String {
    GapReason::try_from(v).map_or(format!("{v}"), |r| {
        r.as_str_name()
            .trim_start_matches("GAP_REASON_")
            .to_ascii_lowercase()
    })
}

fn result_name(v: i32) -> String {
    RecoveryResult::try_from(v).map_or(format!("{v}"), |r| {
        r.as_str_name()
            .trim_start_matches("RECOVERY_RESULT_")
            .to_ascii_lowercase()
    })
}

impl Report {
    pub fn from_store(store: &Store) -> Result<Self, StoreError> {
        let mut r = Self::default();
        for seg in store.segments() {
            let s = r.sources.entry((seg.zone, seg.source.clone())).or_default();
            s.rows += seg.rows;
            s.bytes += std::fs::metadata(store.root().join(&seg.path)).map_or(0, |m| m.len());
            if seg.zone == Zone::Normalized && seg.source == "CLOCK" {
                r.span = Some(match r.span {
                    Some((a, b)) => (a.min(seg.ts_min), b.max(seg.ts_max)),
                    None => (seg.ts_min, seg.ts_max),
                });
            }
        }
        // De a un segmento por vez: la memoria queda acotada al segmento más grande, no a
        // la fuente entera (un día de libro son millones de eventos).
        for seg in store.segments() {
            if seg.zone != Zone::Normalized || seg.source == "CLOCK" {
                continue;
            }
            for e in store.read_segment(seg)? {
                let stats = r
                    .sources
                    .get_mut(&(Zone::Normalized, seg.source.clone()))
                    .expect("fuente");
                match &e.payload {
                    Some(envelope::Payload::Gap(g)) => {
                        stats.gaps += 1;
                        *r.gap_reasons.entry(reason_name(g.reason)).or_default() += 1;
                    }
                    Some(envelope::Payload::GapRecovery(g)) => {
                        *stats.recoveries.entry(result_name(g.result)).or_default() += 1;
                    }
                    _ => {}
                }
            }
        }
        Ok(r)
    }

    /// Bytes por zona y categoría.
    pub fn sizes(&self) -> BTreeMap<(Zone, Category), (u64, u64)> {
        let mut out = BTreeMap::new();
        for ((zone, source), s) in &self.sources {
            let e = out.entry((*zone, Category::of(source))).or_insert((0, 0));
            e.0 += s.rows;
            e.1 += s.bytes;
        }
        out
    }

    /// Reporte en Markdown.
    pub fn markdown(&self) -> String {
        let mb = |b: u64| b as f64 / 1_048_576.0;
        let hours = self
            .span
            .map(|(a, b)| (b - a) as f64 / 3.6e12)
            .filter(|h| *h > 0.0);
        let mut s = String::from("# Reporte de grabación\n\n");
        match hours {
            Some(h) => s.push_str(&format!("Duración (latidos): {h:.2} h\n\n")),
            None => s.push_str("Duración: sin latidos\n\n"),
        }
        s.push_str("## Disco por tipo de dato\n\n| Zona | Tipo | Filas | MB | MB/día |\n|---|---|---:|---:|---:|\n");
        let mut total = 0;
        for ((zone, cat), (rows, bytes)) in self.sizes() {
            total += bytes;
            let per_day = hours.map_or("-".into(), |h| format!("{:.1}", mb(bytes) * 24.0 / h));
            s.push_str(&format!(
                "| {} | {} | {rows} | {:.1} | {per_day} |\n",
                zone.dir_name(),
                cat.name(),
                mb(bytes)
            ));
        }
        let per_day = hours.map_or("-".into(), |h| format!("{:.1}", mb(total) * 24.0 / h));
        s.push_str(&format!(
            "| **total** | | | {:.1} | {per_day} |\n\n",
            mb(total)
        ));
        s.push_str("## Huecos por motivo\n\n");
        if self.gap_reasons.is_empty() {
            s.push_str("Ninguno.\n\n");
        } else {
            for (reason, n) in &self.gap_reasons {
                s.push_str(&format!("- {reason}: {n}\n"));
            }
            s.push('\n');
        }
        s.push_str("## Fuentes\n\n| Zona | Fuente | Filas | MB | Huecos | Recuperaciones |\n|---|---|---:|---:|---:|---|\n");
        for ((zone, source), st) in &self.sources {
            let rec = st
                .recoveries
                .iter()
                .map(|(k, v)| format!("{k}: {v}"))
                .collect::<Vec<_>>()
                .join(", ");
            s.push_str(&format!(
                "| {} | {source} | {} | {:.2} | {} | {rec} |\n",
                zone.dir_name(),
                st.rows,
                mb(st.bytes),
                st.gaps
            ));
        }
        s
    }
}
