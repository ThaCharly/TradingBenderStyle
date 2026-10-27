//! Fecha civil UTC de un instante en ns (algoritmo de Howard Hinnant, `civil_from_days`).

const NS_PER_DAY: i64 = 86_400_000_000_000;

/// Número de día desde el epoch (puede ser negativo).
pub fn day_number(ts_ns: i64) -> i64 {
    ts_ns.div_euclid(NS_PER_DAY)
}

/// "AAAA-MM-DD" del día `days` desde 1970-01-01.
pub fn date_string(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
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
