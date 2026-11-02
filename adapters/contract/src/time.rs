//! Instants as adapters need them: `i64` nanoseconds (Norn's range) and RFC 3339
//! text, exact in both directions, with no dependency.

use prost_types::Timestamp;

const NANOS: i128 = 1_000_000_000;
const DAY: i128 = 86_400;

/// Nanoseconds since the epoch of a wire timestamp; `None` outside Norn's range
/// (about 1677 to 2262), which no stored sample can be.
#[must_use]
pub fn to_nanos(t: &Timestamp) -> Option<i64> {
    i64::try_from(i128::from(t.seconds) * NANOS + i128::from(t.nanos)).ok()
}

/// The wire timestamp of `nanos`, normalised: `nanos` is 0 to 999,999,999 even
/// before the epoch.
#[must_use]
pub fn from_nanos(nanos: i64) -> Timestamp {
    Timestamp {
        seconds: nanos.div_euclid(1_000_000_000),
        nanos: i32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap_or(0),
    }
}

/// Days from 1970-01-01 to a civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * ((month + 9) % 12) + 2) / 5 + day - 1;
    era * 146_097 + yoe * 365 + yoe / 4 - yoe / 100 + doy - 719_468
}

/// The civil date of a day count, the inverse of [`days_from_civil`].
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

/// `nanos` as `2026-11-02T08:00:00.000000000Z`: always nine digits, so every row
/// parses the same way and the text sorts as the time does.
#[must_use]
pub fn format_rfc3339(nanos: i64) -> String {
    let seconds = nanos.div_euclid(1_000_000_000);
    let fraction = nanos.rem_euclid(1_000_000_000);
    let (y, mo, d) = civil_from_days(seconds.div_euclid(86_400));
    let s = seconds.rem_euclid(86_400);
    let (h, mi, s) = (s / 3_600, s / 60 % 60, s % 60);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{fraction:09}Z")
}

fn two(text: &str) -> Option<i64> {
    let valid = text.len() == 2 && text.bytes().all(|b| b.is_ascii_digit());
    valid.then(|| text.parse().ok()).flatten()
}

/// Parses RFC 3339 in UTC (`Z`), with up to nine fractional digits. Refuses a
/// finer fraction, an offset, a date that does not exist or a leap second rather
/// than change the instant, and anything outside Norn's range. Borrows nothing.
#[must_use]
pub fn parse_rfc3339(text: &str) -> Option<i64> {
    let (date, clock) = text.strip_suffix('Z')?.split_once('T')?;
    let (year, rest) = date.split_at_checked(date.len().checked_sub(6)?)?;
    let year: i64 = year.parse().ok()?;
    let (month, day) = rest.strip_prefix('-')?.split_once('-')?;
    let (month, day) = (two(month)?, two(day)?);
    let (clock, fraction) = clock
        .split_once('.')
        .map_or((clock, None), |(c, f)| (c, Some(f)));
    let mut parts = clock.split(':');
    let (h, mi, s) = (
        two(parts.next()?)?,
        two(parts.next()?)?,
        two(parts.next()?)?,
    );
    if parts.next().is_some() || h > 23 || mi > 59 || s > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day);
    if !(1..=12).contains(&month) || civil_from_days(days) != (year, month, day) {
        return None;
    }
    let fraction = match fraction {
        None => 0,
        Some(f) => {
            let digits = f.bytes().all(|b| b.is_ascii_digit()) && (1..=9).contains(&f.len());
            let value: i128 = digits.then(|| f.parse().ok()).flatten()?;
            value * 10i128.pow(9 - u32::try_from(f.len()).ok()?)
        }
    };
    let seconds = i128::from(days) * DAY + i128::from(h * 3_600 + mi * 60 + s);
    i64::try_from(seconds * NANOS + fraction).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_instant_survives_text_both_ways() {
        for nanos in [0, 1, -1, i64::MIN, i64::MAX, 1_793_577_600_123_456_789] {
            assert_eq!(parse_rfc3339(&format_rfc3339(nanos)), Some(nanos));
            assert_eq!(to_nanos(&from_nanos(nanos)), Some(nanos));
        }
    }

    #[test]
    fn text_that_would_change_the_instant_is_refused() {
        for bad in [
            "2026-02-30T00:00:00Z",
            "2026-11-02T08:00:60Z",
            "2026-11-02T08:00:00.1234567891Z",
            "2026-11-02T08:00:00+01:00",
            "2026-11-02",
            "9999-01-01T00:00:00Z",
        ] {
            assert_eq!(parse_rfc3339(bad), None, "{bad}");
        }
    }
}
