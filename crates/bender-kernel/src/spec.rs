//! Especificaciones de instrumento con vigencia (doc 01 §3): `spec(instrumento, as_of)` y
//! la conversión de contratos a unidades del activo base.
//!
//! La conversión la usan los adaptadores (doc 01 §3: "la conversión a contratos vive sólo
//! en el adaptador"), con la spec vigente en el `ts_event` de cada trade. Está en el
//! núcleo para que sea una sola función pura, la misma en vivo y en histórico.

use bender_types::proto::{self, ContractUnit};
use bender_types::{Decimal, DecimalError};

/// Escala del resultado de dividir (contratos inversos): 18 decimales, half-even.
pub const INVERSE_SCALE: u32 = 18;

/// Error de conversión.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SpecError {
    /// No hay spec vigente en ese instante.
    NoSpecAt(i64),
    /// La spec no trae lo necesario para convertir (tamaño o unidad de contrato).
    Incomplete(&'static str),
    Decimal(DecimalError),
}

impl From<DecimalError> for SpecError {
    fn from(e: DecimalError) -> Self {
        Self::Decimal(e)
    }
}

impl std::fmt::Display for SpecError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSpecAt(t) => write!(f, "no hay spec vigente en {t}"),
            Self::Incomplete(what) => write!(f, "spec incompleta: falta {what}"),
            Self::Decimal(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for SpecError {}

/// Versiones de la spec de un instrumento, ordenadas por `valid_from`.
#[derive(Clone, Debug, Default)]
pub struct SpecHistory {
    specs: Vec<proto::InstrumentSpec>,
}

impl SpecHistory {
    pub fn new(mut specs: Vec<proto::InstrumentSpec>) -> Self {
        specs.sort_by_key(|s| s.valid_from);
        Self { specs }
    }

    /// Spec vigente en `ts`: `valid_from ≤ ts < valid_to` (sin `valid_to` = vigente).
    pub fn at(&self, ts: i64) -> Option<&proto::InstrumentSpec> {
        let idx = self.specs.partition_point(|s| s.valid_from <= ts);
        let spec = self.specs.get(idx.checked_sub(1)?)?;
        spec.valid_to.is_none_or(|end| ts < end).then_some(spec)
    }

    /// Cantidad en unidades del activo base de `contracts` contratos negociados a `price`
    /// en `ts_event`, con la spec vigente en ese instante. Spot o derivado que ya cuenta en
    /// base con tamaño 1: la cantidad no cambia.
    ///
    /// - Unidad base: `contratos × contract_size`.
    /// - Unidad quote (inversos): `contratos × contract_size / precio`, con
    ///   [`INVERSE_SCALE`] decimales y redondeo half-even.
    pub fn to_base(
        &self,
        ts_event: i64,
        contracts: Decimal,
        price: Decimal,
    ) -> Result<Decimal, SpecError> {
        let spec = self.at(ts_event).ok_or(SpecError::NoSpecAt(ts_event))?;
        let Some(derivative) = &spec.derivative else {
            return Ok(contracts);
        };
        let size: Decimal = derivative
            .contract_size
            .ok_or(SpecError::Incomplete("contract_size"))?
            .try_into()?;
        let notional = contracts.checked_mul(size)?;
        match ContractUnit::try_from(derivative.contract_unit) {
            Ok(ContractUnit::Base) => Ok(notional),
            Ok(ContractUnit::Quote) => Ok(notional.checked_div_round(price, INVERSE_SCALE)?),
            _ => Err(SpecError::Incomplete("contract_unit")),
        }
    }
}
