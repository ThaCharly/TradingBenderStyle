//! Fecha civil UTC de un instante en ns, para nombrar las particiones por día.

use bender_types::{NS_PER_DAY, civil_from_days};

/// Número de día desde el epoch (puede ser negativo).
pub fn day_number(ts_ns: i64) -> i64 {
    ts_ns.div_euclid(NS_PER_DAY)
}

/// "AAAA-MM-DD" del día `days` desde 1970-01-01.
pub fn date_string(days: i64) -> String {
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_dates() {
        assert_eq!(date_string(0), "1970-01-01");
        assert_eq!(date_string(-1), "1969-12-31");
        assert_eq!(date_string(11_016), "2000-02-29");
        // 2025-10-27T23:59:59.999999999Z y el nanosegundo siguiente.
        let ts = 1_761_609_599_999_999_999;
        assert_eq!(date_string(day_number(ts)), "2025-10-27");
        assert_eq!(date_string(day_number(ts + 1)), "2025-10-28");
        assert_eq!(
            date_string(day_number(1_793_145_600_000_000_000)),
            "2026-10-28"
        );
    }
}
