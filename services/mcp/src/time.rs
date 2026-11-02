//! Times and durations in tool arguments and results: RFC 3339 in UTC, without a
//! dependency. Nanoseconds since the Unix epoch are `i128` here, so arithmetic on
//! them cannot overflow; the daemon bounds what it accepts.

use prost_types::Timestamp;

const NANOS: i128 = 1_000_000_000;

/// Days from 1970-01-01 to a civil date (Howard Hinnant's `days_from_civil`).
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let shifted = (month + 9) % 12;
    let doy = (153 * shifted + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
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

/// A timestamp as `2026-11-02T10:00:00.5Z`, with no fraction when there is none.
pub(crate) fn format(t: Timestamp) -> String {
    let (y, mo, d) = civil_from_days(t.seconds.div_euclid(86_400));
    let s = t.seconds.rem_euclid(86_400);
    let (h, mi, s) = (s / 3_600, s / 60 % 60, s % 60);
    if t.nanos == 0 {
        return format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z");
    }
    let fraction = format!("{:09}", t.nanos);
    let fraction = fraction.trim_end_matches('0');
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{fraction}Z")
}

fn digits(text: &str) -> bool {
    !text.is_empty() && text.bytes().all(|b| b.is_ascii_digit())
}

/// Up to nine fractional digits as nanoseconds; empty is zero.
fn fraction(text: &str) -> Option<i128> {
    if text.is_empty() {
        return Some(0);
    }
    if !digits(text) || text.len() > 9 {
        return None;
    }
    let scale = 10i128.pow(9 - text.len() as u32);
    Some(text.parse::<i128>().ok()? * scale)
}

fn rfc3339(text: &str) -> Option<i128> {
    let (date, clock) = text.strip_suffix('Z')?.split_once('T')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, mo, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, frac) = clock.split_once('.').unwrap_or((clock, ""));
    let mut c = clock.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (h, mi, s) = (c.next()??, c.next()??, c.next()??);
    let in_day = (0..24).contains(&h) && (0..60).contains(&mi) && (0..60).contains(&s);
    let days = days_from_civil(y, mo, day);
    // A date that does not exist, such as 02-31, fails to round-trip.
    (in_day && civil_from_days(days) == (y, mo, day)).then_some(())?;
    let seconds = i128::from(days) * 86_400 + i128::from(h * 3_600 + mi * 60 + s);
    Some(seconds * NANOS + fraction(frac)?)
}

fn unix_seconds(text: &str) -> Option<i128> {
    let (whole, frac) = text.split_once('.').unwrap_or((text, ""));
    digits(whole).then_some(())?;
    Some(whole.parse::<i128>().ok()?.checked_mul(NANOS)? + fraction(frac)?)
}

/// A duration such as `500ms`, `10s`, `5m`, `1h` or `1d`, in nanoseconds.
pub(crate) fn duration(text: &str) -> Option<i128> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let nanos: i128 = match unit {
        "ns" => 1,
        "us" => 1_000,
        "ms" => 1_000_000,
        "s" => NANOS,
        "m" => 60 * NANOS,
        "h" => 3_600 * NANOS,
        "d" => 86_400 * NANOS,
        _ => return None,
    };
    number.parse::<i128>().ok()?.checked_mul(nanos)
}

/// A time: RFC 3339 in UTC, Unix seconds, `now`, or `now-1h`. `now` is handed in,
/// so every page of one read agrees on what it meant.
pub(crate) fn instant(text: &str, now: i128) -> Option<i128> {
    if text == "now" {
        return Some(now);
    }
    if let Some(ago) = text.strip_prefix("now-") {
        return Some(now - duration(ago)?);
    }
    unix_seconds(text).or_else(|| rfc3339(text))
}

/// Nanoseconds as the wire's timestamp; `None` when they do not fit it.
pub(crate) fn timestamp(nanos: i128) -> Option<Timestamp> {
    Some(Timestamp {
        seconds: i64::try_from(nanos.div_euclid(NANOS)).ok()?,
        nanos: i32::try_from(nanos.rem_euclid(NANOS)).ok()?,
    })
}

/// A wire timestamp as nanoseconds.
pub(crate) fn nanos(t: Timestamp) -> i128 {
    i128::from(t.seconds) * NANOS + i128::from(t.nanos)
}

/// Nanoseconds as the wire's duration; `None` when negative or too long.
pub(crate) fn wire_duration(nanos: i128) -> Option<prost_types::Duration> {
    if nanos < 0 {
        return None;
    }
    Some(prost_types::Duration {
        seconds: i64::try_from(nanos / NANOS).ok()?,
        nanos: i32::try_from(nanos % NANOS).ok()?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_round_trips_through_its_text() {
        for (seconds, nanos) in [(0, 0), (1_793_568_259, 490_508_000), (951_782_400, 1)] {
            let t = Timestamp { seconds, nanos };
            assert_eq!(rfc3339(&format(t)), Some(super::nanos(t)), "{}", format(t));
        }
    }

    #[test]
    fn relative_times_and_durations_parse_and_nonsense_does_not() {
        assert_eq!(duration("5m"), Some(300_000_000_000));
        assert_eq!(instant("now-1s", 2_000_000_000), Some(1_000_000_000));
        assert_eq!(instant("12.5", 0), Some(12_500_000_000));
        assert_eq!(instant("yesterday", 0), None);
        assert_eq!(duration("5"), None);
    }

    #[test]
    fn a_date_that_does_not_exist_and_an_offset_are_refused() {
        assert_eq!(rfc3339("2026-13-01T00:00:00Z"), None);
        assert_eq!(rfc3339("2026-02-31T00:00:00Z"), None);
        assert_eq!(rfc3339("2026-01-01T00:00:00+01:00"), None);
        assert_eq!(rfc3339("2026-01-01T00:00:00.1234567891Z"), None);
    }
}
