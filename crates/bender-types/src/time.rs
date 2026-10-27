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

/// Nanosegundos en un día.
pub const NS_PER_DAY: i64 = 86_400_000_000_000;

/// Fecha civil UTC (año, mes 1–12, día 1–31) del día número `days` desde 1970-01-01
/// (algoritmo `civil_from_days` de Howard Hinnant; vale también para días negativos).
pub fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// Inverso de [`civil_from_days`]: número de día de la fecha civil UTC.
pub fn days_from_civil(year: i64, month: u32, day: u32) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let m = i64::from(month);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + i64::from(day) - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod civil_tests {
    use super::*;

    #[test]
    fn civil_roundtrip_and_known_dates() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        assert_eq!(civil_from_days(-1), (1969, 12, 31));
        assert_eq!(civil_from_days(11_016), (2000, 2, 29));
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        for days in -800_000..800_000 {
            let (y, m, d) = civil_from_days(days);
            assert_eq!(days_from_civil(y, m, d), days);
        }
    }
}
