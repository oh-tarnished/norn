//! Times and durations on the command line: RFC 3339 in UTC, without a dependency.

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

/// `seconds` and `nanos` since the epoch as `2026-11-02T10:00:00.5Z`, with no
/// fraction when there is none.
pub(crate) fn format(seconds: i64, nanos: i32) -> String {
    let (y, mo, d) = civil_from_days(seconds.div_euclid(86_400));
    let s = seconds.rem_euclid(86_400);
    let (h, mi, s) = (s / 3_600, s / 60 % 60, s % 60);
    if nanos == 0 {
        return format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z");
    }
    let fraction = format!("{nanos:09}");
    let fraction = fraction.trim_end_matches('0');
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{fraction}Z")
}

fn rfc3339(text: &str) -> Option<(i64, i32)> {
    let body = text.strip_suffix('Z')?;
    let (date, clock) = body.split_once('T')?;
    let mut d = date.splitn(3, '-').map(|p| p.parse::<i64>().ok());
    let (y, mo, day) = (d.next()??, d.next()??, d.next()??);
    let (clock, fraction) = clock.split_once('.').unwrap_or((clock, ""));
    let mut c = clock.splitn(3, ':').map(|p| p.parse::<i64>().ok());
    let (h, mi, s) = (c.next()??, c.next()??, c.next()??);
    let valid = (1..=12).contains(&mo) && (1..=31).contains(&day);
    (valid && h < 24 && mi < 60 && s < 61).then_some(())?;
    let digits = fraction.len().min(9);
    let nanos = if fraction.is_empty() {
        0
    } else {
        fraction.get(..digits)?.parse::<i32>().ok()? * 10i32.pow(9 - digits as u32)
    };
    let seconds = days_from_civil(y, mo, day) * 86_400 + h * 3_600 + mi * 60 + s;
    Some((seconds, nanos))
}

/// A duration such as `500ms`, `10s`, `5m`, `1h` or `1d`, in nanoseconds.
pub(crate) fn duration(text: &str) -> Option<i128> {
    let split = text.find(|c: char| !c.is_ascii_digit())?;
    let (number, unit) = text.split_at(split);
    let nanos: i128 = match unit {
        "ns" => 1,
        "us" => 1_000,
        "ms" => 1_000_000,
        "s" => 1_000_000_000,
        "m" => 60_000_000_000,
        "h" => 3_600_000_000_000,
        "d" => 86_400_000_000_000,
        _ => return None,
    };
    number.parse::<i128>().ok()?.checked_mul(nanos)
}

fn parts(nanos: i128) -> Option<(i64, i32)> {
    let seconds = i64::try_from(nanos.div_euclid(1_000_000_000)).ok()?;
    Some((
        seconds,
        i32::try_from(nanos.rem_euclid(1_000_000_000)).ok()?,
    ))
}

/// A time: RFC 3339 in UTC, Unix seconds, `now`, or `now-1h`. `now` is the
/// wall clock's nanoseconds since the epoch, handed in.
pub(crate) fn instant(text: &str, now: i128) -> Option<(i64, i32)> {
    if text == "now" {
        return parts(now);
    }
    if let Some(ago) = text.strip_prefix("now-") {
        return parts(now - duration(ago)?);
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return parts((seconds * 1e9) as i128);
    }
    rfc3339(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_time_round_trips_through_its_text() {
        for (s, n) in [
            (0, 0),
            (1_793_568_259, 490_508_000),
            (-1, 5),
            (951_782_400, 0),
        ] {
            assert_eq!(rfc3339(&format(s, n)), Some((s, n)), "{}", format(s, n));
        }
        assert_eq!(format(1_793_664_000, 0), "2026-11-03T00:00:00Z");
    }

    #[test]
    fn relative_times_and_durations_parse_and_nonsense_does_not() {
        assert_eq!(duration("5m"), Some(300_000_000_000));
        assert_eq!(instant("now-1s", 2_000_000_000), Some((1, 0)));
        assert_eq!(instant("12.5", 0), Some((12, 500_000_000)));
        assert_eq!(instant("yesterday", 0), None);
        assert_eq!(duration("5"), None);
        assert_eq!(rfc3339("2026-13-01T00:00:00Z"), None);
    }
}
