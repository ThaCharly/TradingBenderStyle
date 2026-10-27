//! Tiempo del dominio (doc 01 §4): instantes y duraciones en nanosegundos UTC, `i64`.

use std::fmt;
use std::ops::{Add, Sub};

/// Instante en nanosegundos UTC desde el epoch Unix. Orden total por valor.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct TimestampNs(pub i64);

/// Duración en nanosegundos (puede ser negativa).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DurationNs(pub i64);

impl DurationNs {
    pub const ZERO: Self = Self(0);

    pub const fn from_nanos(ns: i64) -> Self {
        Self(ns)
    }

    pub const fn from_millis(ms: i64) -> Self {
        Self(ms * 1_000_000)
    }

    pub const fn from_secs(s: i64) -> Self {
        Self(s * 1_000_000_000)
    }

    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, other: Self) -> Option<Self> {
        self.0.checked_add(other.0).map(Self)
    }

    pub fn checked_mul(self, factor: i64) -> Option<Self> {
        self.0.checked_mul(factor).map(Self)
    }
}

impl TimestampNs {
    pub const fn from_nanos(ns: i64) -> Self {
        Self(ns)
    }

    pub const fn as_nanos(self) -> i64 {
        self.0
    }

    pub fn checked_add(self, d: DurationNs) -> Option<Self> {
        self.0.checked_add(d.0).map(Self)
    }

    pub fn checked_sub(self, d: DurationNs) -> Option<Self> {
        self.0.checked_sub(d.0).map(Self)
    }

    /// Duración `self − earlier`.
    pub fn checked_duration_since(self, earlier: Self) -> Option<DurationNs> {
        self.0.checked_sub(earlier.0).map(DurationNs)
    }

    /// Inicio del intervalo de largo `step` que contiene a `self`, alineado al epoch
    /// (también para instantes anteriores a 1970). `None` si `step` ≤ 0.
    pub fn floor_to(self, step: DurationNs) -> Option<Self> {
        (step.0 > 0).then(|| Self(self.0.div_euclid(step.0) * step.0))
    }
}

/// Las sumas y restas con operadores desbordan con pánico en debug y envuelven en
/// release, como `i64`; donde el desborde sea posible, usar los métodos `checked_*`.
impl Add<DurationNs> for TimestampNs {
    type Output = Self;
    fn add(self, d: DurationNs) -> Self {
        Self(self.0 + d.0)
    }
}

impl Sub<DurationNs> for TimestampNs {
    type Output = Self;
    fn sub(self, d: DurationNs) -> Self {
        Self(self.0 - d.0)
    }
}

impl Sub for TimestampNs {
    type Output = DurationNs;
    fn sub(self, other: Self) -> DurationNs {
        DurationNs(self.0 - other.0)
    }
}

impl Add for DurationNs {
    type Output = Self;
    fn add(self, other: Self) -> Self {
        Self(self.0 + other.0)
    }
}

impl Sub for DurationNs {
    type Output = Self;
    fn sub(self, other: Self) -> Self {
        Self(self.0 - other.0)
    }
}

impl fmt::Display for TimestampNs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

impl fmt::Display for DurationNs {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}ns", self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn arithmetic_and_order() {
        let t = TimestampNs(1_000);
        let d = DurationNs::from_nanos(250);
        assert_eq!(t + d, TimestampNs(1_250));
        assert_eq!(t - d, TimestampNs(750));
        assert_eq!((t + d) - t, d);
        assert!(t < t + d);
        assert_eq!(DurationNs::from_secs(2), DurationNs::from_millis(2_000));
        assert_eq!(TimestampNs(i64::MAX).checked_add(d), None);
    }

    #[test]
    fn floor_is_epoch_aligned_also_before_1970() {
        let h4 = DurationNs::from_secs(4 * 3600);
        let t = TimestampNs(DurationNs::from_secs(5 * 3600 + 7).as_nanos());
        assert_eq!(t.floor_to(h4), Some(TimestampNs(h4.as_nanos())));
        assert_eq!(
            TimestampNs(-1).floor_to(h4),
            Some(TimestampNs(-h4.as_nanos()))
        );
        assert_eq!(t.floor_to(DurationNs::ZERO), None);
    }
}
