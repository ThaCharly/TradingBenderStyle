//! Decimal de punto fijo (Pilar 1): mantisa `i128` y escala. Conversión sin pérdida con el
//! mensaje `bender.v1.Decimal`.

use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::str::FromStr;

use crate::proto;

/// Escala máxima admitida: 10^38 es la mayor potencia de 10 que cabe en `i128`.
pub const MAX_SCALE: u32 = 38;

/// Número decimal exacto: `mantissa × 10^(−scale)`.
///
/// Conserva la escala con la que se construyó (1.50 se muestra "1.50"), pero la igualdad,
/// el orden y el hash son numéricos: `1.5 == 1.50`.
#[derive(Clone, Copy, Debug)]
pub struct Decimal {
    mantissa: i128,
    scale: u32,
}

/// Errores al construir o convertir un [`Decimal`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DecimalError {
    /// La escala supera [`MAX_SCALE`].
    ScaleTooLarge(u32),
    /// El texto no es un decimal válido.
    Parse(String),
    /// El resultado no cabe en una mantisa de 128 bits.
    Overflow,
    DivisionByZero,
}

impl fmt::Display for DecimalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScaleTooLarge(s) => write!(f, "escala {s} mayor que {MAX_SCALE}"),
            Self::Parse(s) => write!(f, "decimal inválido: {s:?}"),
            Self::Overflow => write!(f, "desborde de la mantisa de 128 bits"),
            Self::DivisionByZero => write!(f, "división por cero"),
        }
    }
}

impl std::error::Error for DecimalError {}

fn pow10(exp: u32) -> Option<i128> {
    10i128.checked_pow(exp)
}

impl Decimal {
    pub const ZERO: Self = Self {
        mantissa: 0,
        scale: 0,
    };

    /// `mantissa × 10^(−scale)`.
    pub fn new(mantissa: i128, scale: u32) -> Result<Self, DecimalError> {
        if scale > MAX_SCALE {
            return Err(DecimalError::ScaleTooLarge(scale));
        }
        Ok(Self { mantissa, scale })
    }

    pub fn mantissa(self) -> i128 {
        self.mantissa
    }

    pub fn scale(self) -> u32 {
        self.scale
    }

    pub fn is_zero(self) -> bool {
        self.mantissa == 0
    }

    pub fn is_negative(self) -> bool {
        self.mantissa < 0
    }

    /// Mismo valor con la menor escala posible (sin ceros a la derecha).
    pub fn normalized(self) -> Self {
        let (mut mantissa, mut scale) = (self.mantissa, self.scale);
        while scale > 0 && mantissa % 10 == 0 {
            mantissa /= 10;
            scale -= 1;
        }
        Self { mantissa, scale }
    }

    /// Mismo valor con escala `scale` (≥ la actual). Falla si no cabe.
    pub fn rescaled(self, scale: u32) -> Result<Self, DecimalError> {
        if scale > MAX_SCALE {
            return Err(DecimalError::ScaleTooLarge(scale));
        }
        if scale < self.scale {
            let n = self.normalized();
            if n.scale > scale {
                return Err(DecimalError::Overflow);
            }
            return n.rescaled(scale);
        }
        let factor = pow10(scale - self.scale).ok_or(DecimalError::Overflow)?;
        let mantissa = self
            .mantissa
            .checked_mul(factor)
            .ok_or(DecimalError::Overflow)?;
        Ok(Self { mantissa, scale })
    }

    fn aligned(self, other: Self) -> Result<(i128, i128, u32), DecimalError> {
        let scale = self.scale.max(other.scale);
        Ok((
            self.rescaled(scale)?.mantissa,
            other.rescaled(scale)?.mantissa,
            scale,
        ))
    }

    pub fn checked_add(self, other: Self) -> Result<Self, DecimalError> {
        if self.scale == other.scale {
            let mantissa = self
                .mantissa
                .checked_add(other.mantissa)
                .ok_or(DecimalError::Overflow)?;
            return Ok(Self { mantissa, ..self });
        }
        let (a, b, scale) = self.aligned(other)?;
        let mantissa = a.checked_add(b).ok_or(DecimalError::Overflow)?;
        Ok(Self { mantissa, scale })
    }

    pub fn checked_sub(self, other: Self) -> Result<Self, DecimalError> {
        self.checked_add(other.checked_neg()?)
    }

    /// Producto exacto: la escala es la suma de las escalas.
    pub fn checked_mul(self, other: Self) -> Result<Self, DecimalError> {
        let mantissa = self
            .mantissa
            .checked_mul(other.mantissa)
            .ok_or(DecimalError::Overflow)?;
        Self::new(mantissa, self.scale + other.scale)
    }

    /// Cociente `self / other` con escala `scale`, redondeado al par más cercano
    /// (*half-even*, sin sesgo). Falla si `other` es cero o si no cabe.
    pub fn checked_div_round(self, other: Self, scale: u32) -> Result<Self, DecimalError> {
        if scale > MAX_SCALE {
            return Err(DecimalError::ScaleTooLarge(scale));
        }
        if other.mantissa == 0 {
            return Err(DecimalError::DivisionByZero);
        }
        // resultado = (a / 10^sa) / (b / 10^sb) · 10^s = a · 10^(s + sb − sa) / b
        let exp = i64::from(scale) + i64::from(other.scale) - i64::from(self.scale);
        let (num, den) = if exp >= 0 {
            let f = pow10(exp as u32).ok_or(DecimalError::Overflow)?;
            (
                self.mantissa.checked_mul(f).ok_or(DecimalError::Overflow)?,
                other.mantissa,
            )
        } else {
            let f = pow10((-exp) as u32).ok_or(DecimalError::Overflow)?;
            (
                self.mantissa,
                other
                    .mantissa
                    .checked_mul(f)
                    .ok_or(DecimalError::Overflow)?,
            )
        };
        let mut q = num / den;
        let r = num % den;
        if r != 0 {
            // |2r| vs |den|, sin desbordar.
            let (r_abs, d_abs) = (r.unsigned_abs(), den.unsigned_abs());
            let twice = r_abs.checked_mul(2);
            let round_away = match twice {
                None => true,
                Some(t) => t > d_abs || (t == d_abs && q % 2 != 0),
            };
            if round_away {
                let negative = (num < 0) != (den < 0);
                q = if negative { q - 1 } else { q + 1 };
            }
        }
        Self::new(q, scale)
    }

    pub fn checked_neg(self) -> Result<Self, DecimalError> {
        let mantissa = self.mantissa.checked_neg().ok_or(DecimalError::Overflow)?;
        Ok(Self { mantissa, ..self })
    }

    pub fn abs(self) -> Result<Self, DecimalError> {
        if self.is_negative() {
            self.checked_neg()
        } else {
            Ok(self)
        }
    }
}

impl Default for Decimal {
    fn default() -> Self {
        Self::ZERO
    }
}

impl PartialEq for Decimal {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Decimal {}

impl Ord for Decimal {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.scale == other.scale {
            return self.mantissa.cmp(&other.mantissa);
        }
        let (a, b) = (self.normalized(), other.normalized());
        match a.aligned(b) {
            Ok((x, y, _)) => x.cmp(&y),
            // Si llevar uno a la escala del otro desborda, ese tiene más magnitud: alcanza
            // con su signo para decidir.
            Err(_) => {
                let a_bigger = a.scale < b.scale;
                let (big, sign) = if a_bigger {
                    (a.mantissa, Ordering::Greater)
                } else {
                    (b.mantissa, Ordering::Less)
                };
                if big > 0 { sign } else { sign.reverse() }
            }
        }
    }
}

impl PartialOrd for Decimal {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for Decimal {
    fn hash<H: Hasher>(&self, state: &mut H) {
        let n = self.normalized();
        n.mantissa.hash(state);
        n.scale.hash(state);
    }
}

impl fmt::Display for Decimal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let digits = self.mantissa.unsigned_abs().to_string();
        let sign = if self.mantissa < 0 { "-" } else { "" };
        let scale = self.scale as usize;
        if scale == 0 {
            return write!(f, "{sign}{digits}");
        }
        let padded = format!("{digits:0>width$}", width = scale + 1);
        let (int, frac) = padded.split_at(padded.len() - scale);
        write!(f, "{sign}{int}.{frac}")
    }
}

impl FromStr for Decimal {
    type Err = DecimalError;

    /// Acepta `[-+]dígitos[.dígitos]`; conserva la escala escrita ("1.50" → escala 2).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let err = || DecimalError::Parse(s.to_string());
        let (negative, body) = match s.as_bytes().first() {
            Some(b'-') => (true, &s[1..]),
            Some(b'+') => (false, &s[1..]),
            _ => (false, s),
        };
        let (int, frac) = body.split_once('.').unwrap_or((body, ""));
        if int.is_empty() || !int.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit()) {
            return Err(err());
        }
        if body.contains('.') && frac.is_empty() {
            return Err(err());
        }
        let scale = u32::try_from(frac.len()).map_err(|_| err())?;
        if scale > MAX_SCALE {
            return Err(DecimalError::ScaleTooLarge(scale));
        }
        let magnitude: u128 = format!("{int}{frac}")
            .parse()
            .map_err(|_| DecimalError::Overflow)?;
        let mantissa = if negative {
            0i128
                .checked_sub_unsigned(magnitude)
                .ok_or(DecimalError::Overflow)?
        } else {
            i128::try_from(magnitude).map_err(|_| DecimalError::Overflow)?
        };
        Ok(Self { mantissa, scale })
    }
}

impl From<Decimal> for proto::Decimal {
    fn from(d: Decimal) -> Self {
        let bits = d.mantissa as u128;
        Self {
            mantissa_lo: bits as u64,
            mantissa_hi: (bits >> 64) as i64,
            scale: d.scale,
        }
    }
}

impl TryFrom<proto::Decimal> for Decimal {
    type Error = DecimalError;

    /// Sin pérdida: la mantisa de 128 bits del mensaje es exactamente un `i128`. Sólo
    /// falla si la escala supera [`MAX_SCALE`].
    fn try_from(p: proto::Decimal) -> Result<Self, Self::Error> {
        let bits = ((p.mantissa_hi as u64 as u128) << 64) | p.mantissa_lo as u128;
        Self::new(bits as i128, p.scale)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn d(s: &str) -> Decimal {
        s.parse().unwrap()
    }

    #[test]
    fn parses_and_displays_keeping_scale() {
        for s in [
            "0",
            "1.50",
            "-0.001",
            "88000.12345678",
            "-12",
            "0.000000000000000001",
        ] {
            assert_eq!(d(s).to_string(), s);
        }
        assert_eq!(d("+3.0").to_string(), "3.0");
        for bad in ["", "-", ".5", "5.", "1.2.3", "1e5", "abc", " 1"] {
            assert!(bad.parse::<Decimal>().is_err(), "{bad:?}");
        }
    }

    #[test]
    fn equality_order_and_hash_are_numeric() {
        use std::collections::HashSet;
        assert_eq!(d("1.5"), d("1.50"));
        assert!(d("-0.1") < d("0"));
        assert!(d("2") > d("1.999999"));
        let set: HashSet<_> = [d("1.5"), d("1.500"), d("2")].into_iter().collect();
        assert_eq!(set.len(), 2);
        // Comparación que desborda al alinear escalas.
        let huge = Decimal::new(i128::MAX, 0).unwrap();
        let tiny = Decimal::new(1, 38).unwrap();
        assert!(huge > tiny && tiny < huge);
        assert!(huge.checked_neg().unwrap() < tiny);
    }

    #[test]
    fn arithmetic_is_exact() {
        assert_eq!(d("0.1").checked_add(d("0.2")).unwrap(), d("0.3"));
        assert_eq!(d("1").checked_sub(d("1.25")).unwrap(), d("-0.25"));
        let quote = d("88000.5").checked_mul(d("0.002")).unwrap();
        assert_eq!(quote.to_string(), "176.0010");
        assert_eq!(
            Decimal::new(i128::MAX, 0).unwrap().checked_add(d("1")),
            Err(DecimalError::Overflow)
        );
        assert_eq!(Decimal::new(1, 39), Err(DecimalError::ScaleTooLarge(39)));
    }

    #[test]
    fn division_rounds_half_even() {
        let div = |a: &str, b: &str, s| d(a).checked_div_round(d(b), s).unwrap().to_string();
        assert_eq!(div("1", "3", 4), "0.3333");
        assert_eq!(div("2", "3", 4), "0.6667");
        assert_eq!(div("-2", "3", 4), "-0.6667");
        assert_eq!(div("0.25", "1", 1), "0.2"); // mitad → par
        assert_eq!(div("0.35", "1", 1), "0.4");
        assert_eq!(div("-0.25", "1", 1), "-0.2");
        assert_eq!(div("100", "65000.5", 18), "0.001538449704233044");
        assert_eq!(div("10", "0.04", 0), "250");
        assert_eq!(
            d("1").checked_div_round(Decimal::ZERO, 2),
            Err(DecimalError::DivisionByZero)
        );
    }

    #[test]
    fn proto_layout_is_twos_complement_i128() {
        let p = proto::Decimal::from(d("-1"));
        assert_eq!((p.mantissa_hi, p.mantissa_lo, p.scale), (-1, u64::MAX, 0));
        let p = proto::Decimal::from(Decimal::new(1i128 << 64, 2).unwrap());
        assert_eq!((p.mantissa_hi, p.mantissa_lo, p.scale), (1, 0, 2));
    }

    proptest! {
        /// Criterio de #19: ida y vuelta proto ↔ Rust sin pérdida para escalas 0–18.
        #[test]
        fn proto_roundtrip_is_lossless(mantissa in any::<i128>(), scale in 0u32..=18) {
            let value = Decimal::new(mantissa, scale).unwrap();
            let back = Decimal::try_from(proto::Decimal::from(value)).unwrap();
            prop_assert_eq!(back.mantissa(), mantissa);
            prop_assert_eq!(back.scale(), scale);
        }

        #[test]
        fn text_roundtrip_is_lossless(mantissa in any::<i128>(), scale in 0u32..=18) {
            let value = Decimal::new(mantissa, scale).unwrap();
            let back: Decimal = value.to_string().parse().unwrap();
            prop_assert_eq!((back.mantissa(), back.scale()), (mantissa, scale));
        }

        #[test]
        fn order_matches_rescaled_mantissas(
            a in -1_000_000_000i64..1_000_000_000, sa in 0u32..=18,
            b in -1_000_000_000i64..1_000_000_000, sb in 0u32..=18,
        ) {
            let (x, y) = (Decimal::new(a.into(), sa).unwrap(), Decimal::new(b.into(), sb).unwrap());
            let s = sa.max(sb);
            let (mx, my) = (x.rescaled(s).unwrap().mantissa(), y.rescaled(s).unwrap().mantissa());
            prop_assert_eq!(x.cmp(&y), mx.cmp(&my));
        }
    }
}
