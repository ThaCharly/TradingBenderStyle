//! Identificadores internos ULID (doc 01 §2), con conversión al mensaje `bender.v1.Ulid`.

use std::fmt;
use std::str::FromStr;

use crate::proto;

/// ULID: 128 bits ordenables por tiempo. Se construye desde un instante y aleatoriedad
/// provistos por quien llama; este crate nunca lee el reloj.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ulid(ulid::Ulid);

/// Errores al convertir un [`Ulid`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UlidError {
    /// El mensaje no trae exactamente 16 bytes.
    Length(usize),
    /// Texto que no es un ULID en Crockford base32.
    Parse(String),
}

impl fmt::Display for UlidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Length(n) => write!(f, "un ULID ocupa 16 bytes, no {n}"),
            Self::Parse(s) => write!(f, "ULID inválido: {s:?}"),
        }
    }
}

impl std::error::Error for UlidError {}

impl Ulid {
    /// Desde milisegundos Unix (48 bits) y 80 bits de aleatoriedad.
    pub fn from_parts(timestamp_ms: u64, random: u128) -> Self {
        Self(ulid::Ulid::from_parts(timestamp_ms, random))
    }

    pub fn from_bytes(bytes: [u8; 16]) -> Self {
        Self(ulid::Ulid::from_bytes(bytes))
    }

    /// 16 bytes big-endian, el formato del mensaje.
    pub fn to_bytes(self) -> [u8; 16] {
        self.0.to_bytes()
    }

    pub fn timestamp_ms(self) -> u64 {
        self.0.timestamp_ms()
    }
}

impl fmt::Display for Ulid {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.0, f)
    }
}

impl FromStr for Ulid {
    type Err = UlidError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        ulid::Ulid::from_string(s)
            .map(Self)
            .map_err(|_| UlidError::Parse(s.to_string()))
    }
}

impl From<Ulid> for proto::Ulid {
    fn from(id: Ulid) -> Self {
        Self {
            value: id.to_bytes().to_vec(),
        }
    }
}

impl TryFrom<proto::Ulid> for Ulid {
    type Error = UlidError;

    fn try_from(p: proto::Ulid) -> Result<Self, Self::Error> {
        let bytes: [u8; 16] = p
            .value
            .as_slice()
            .try_into()
            .map_err(|_| UlidError::Length(p.value.len()))?;
        Ok(Self::from_bytes(bytes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrips_through_proto_and_text() {
        let id = Ulid::from_parts(1_700_000_000_000, 0x1234_5678_9abc_def0_1234);
        assert_eq!(Ulid::try_from(proto::Ulid::from(id)).unwrap(), id);
        assert_eq!(id.to_string().parse::<Ulid>().unwrap(), id);
        assert_eq!(id.timestamp_ms(), 1_700_000_000_000);
    }

    #[test]
    fn orders_by_time_and_rejects_bad_input() {
        assert!(Ulid::from_parts(1, u128::MAX >> 48) < Ulid::from_parts(2, 0));
        let short = proto::Ulid { value: vec![0; 15] };
        assert_eq!(Ulid::try_from(short), Err(UlidError::Length(15)));
        assert!("no-es-un-ulid".parse::<Ulid>().is_err());
    }
}
