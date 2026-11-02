//! Calendar months in UTC, for retention. The proleptic Gregorian conversions are
//! Howard Hinnant's `days_from_civil` and `civil_from_days`.

use norn_core::Timestamp;

const DAY_NANOS: i64 = 86_400_000_000_000;

fn is_leap(y: i64) -> bool {
    y % 4 == 0 && (y % 100 != 0 || y % 400 == 0)
}

fn month_days(y: i64, m: i64) -> i64 {
    match m {
        2 if is_leap(y) => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    (y, m, d)
}

/// `t` moved back `months` calendar months: the same day and time of day, or the
/// month's last day when it has no such day. Pure.
#[must_use]
pub(crate) fn months_before(t: Timestamp, months: u32) -> Timestamp {
    let nanos = t.as_nanos();
    let (days, time) = (nanos.div_euclid(DAY_NANOS), nanos.rem_euclid(DAY_NANOS));
    let (y, m, d) = civil_from_days(days);
    let index = y * 12 + (m - 1) - i64::from(months);
    let (year, month) = (index.div_euclid(12), index.rem_euclid(12) + 1);
    let day = d.min(month_days(year, month));
    let back = days_from_civil(year, month, day).saturating_mul(DAY_NANOS);
    Timestamp::from_nanos(back.saturating_add(time))
}

/// The minute of the UTC day `t` falls in, 0 to 1,439. Pure.
#[must_use]
pub(crate) fn minute_of_day(t: Timestamp) -> u16 {
    let minute = t.as_nanos().rem_euclid(DAY_NANOS) / 60_000_000_000;
    u16::try_from(minute).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{days_from_civil, minute_of_day, months_before};
    use norn_core::Timestamp;

    fn at(y: i64, m: i64, d: i64) -> Timestamp {
        Timestamp::from_nanos(days_from_civil(y, m, d) * super::DAY_NANOS)
    }

    #[test]
    fn the_unix_epoch_is_day_zero() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
    }

    #[test]
    fn a_month_back_keeps_the_day_of_the_month() {
        assert_eq!(months_before(at(2026, 3, 15), 1), at(2026, 2, 15));
    }

    #[test]
    fn a_day_the_earlier_month_lacks_becomes_its_last_day() {
        assert_eq!(months_before(at(2026, 3, 31), 1), at(2026, 2, 28));
        assert_eq!(months_before(at(2024, 3, 31), 1), at(2024, 2, 29));
    }

    #[test]
    fn months_cross_a_year_boundary() {
        assert_eq!(months_before(at(2026, 1, 10), 14), at(2024, 11, 10));
    }

    #[test]
    fn the_minute_of_the_day_ignores_the_date() {
        let t = Timestamp::from_nanos(at(2026, 5, 1).as_nanos() + 90 * 60_000_000_000);
        assert_eq!(minute_of_day(t), 90);
    }
}
