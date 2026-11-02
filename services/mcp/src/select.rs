//! Turning a tool's arguments into a request's selection and interval.

use norn_protocol::google::r#type::Interval;
use prost_types::Timestamp;

use crate::error::ToolError;
use crate::time;

/// Whose signals: the `parent` and `filter` every read takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Selection {
    pub(crate) parent: String,
    pub(crate) filter: String,
}

/// A resource ID. Refusing anything else keeps a target from reaching into the filter
/// it is quoted in. `-` alone is the AIP-159 wildcard, which is why it passes.
fn id(argument: &str, text: &str) -> Result<(), ToolError> {
    let ok = !text.is_empty()
        && text.len() <= 63
        && text
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if ok {
        return Ok(());
    }
    Err(ToolError::invalid(
        argument,
        "an ID is lower-case letters, digits and hyphens",
    ))
}

/// A `target` and the optional signal filter `select`.
pub(crate) fn selection(target: &str, select: Option<&str>) -> Result<Selection, ToolError> {
    let (parent, mut filter) = if target == "all" {
        ("entities/-".to_owned(), String::new())
    } else {
        let bare = target.strip_prefix("entities/").unwrap_or(target);
        match bare.split_once("/signals/") {
            Some((entity, signal)) => {
                id("target", entity)?;
                id("target", signal)?;
                (format!("entities/{entity}"), format!("id = \"{signal}\""))
            }
            None => {
                id("target", bare)?;
                (format!("entities/{bare}"), String::new())
            }
        }
    };
    if let Some(select) = select.filter(|s| !s.is_empty()) {
        if !filter.is_empty() {
            filter.push_str(" AND ");
        }
        filter.push_str(select);
    }
    Ok(Selection { parent, filter })
}

/// A required time argument.
pub(crate) fn instant(argument: &str, text: &str, now: i128) -> Result<Timestamp, ToolError> {
    time::instant(text, now)
        .and_then(time::timestamp)
        .ok_or_else(|| {
            ToolError::invalid(argument, "use RFC 3339 in UTC, Unix seconds, now or now-1h")
        })
}

/// A half-open interval from optional bounds; `None` when both are open.
pub(crate) fn interval(
    from: Option<&str>,
    to: Option<&str>,
    now: i128,
) -> Result<Option<Interval>, ToolError> {
    let bound = |name, text: Option<&str>| text.map(|t| instant(name, t, now)).transpose();
    let interval = Interval {
        start_time: bound("from", from)?,
        end_time: bound("to", to)?,
    };
    Ok((interval != Interval::default()).then_some(interval))
}

/// A duration argument, as nanoseconds.
pub(crate) fn duration(argument: &str, text: &str) -> Result<i128, ToolError> {
    time::duration(text)
        .ok_or_else(|| ToolError::invalid(argument, "use a duration like 500ms, 10s, 5m, 1h or 1d"))
}

/// A duration argument, for the wire.
pub(crate) fn wire_duration(
    argument: &str,
    text: &str,
) -> Result<prost_types::Duration, ToolError> {
    time::wire_duration(duration(argument, text)?)
        .ok_or_else(|| ToolError::invalid(argument, "this duration is too long"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_kind_of_target_selects_what_it_names() {
        let s = selection("all", None).unwrap();
        assert_eq!((s.parent.as_str(), s.filter.as_str()), ("entities/-", ""));
        for target in ["ahu", "entities/ahu"] {
            assert_eq!(selection(target, None).unwrap().parent, "entities/ahu");
        }
        let s = selection("entities/ahu/signals/t", Some("unit = \"Cel\"")).unwrap();
        assert_eq!(s.parent, "entities/ahu");
        assert_eq!(s.filter, "id = \"t\" AND unit = \"Cel\"");
        let s = selection("entities/-/signals/t", None).unwrap();
        assert_eq!(s.parent, "entities/-");
    }

    #[test]
    fn a_target_cannot_reach_into_the_filter_it_is_quoted_in() {
        assert!(selection("entities/a/signals/x\" OR id = \"y", None).is_err());
        assert!(selection("Upper", None).is_err());
        assert!(selection("", None).is_err());
    }

    #[test]
    fn open_bounds_give_no_interval_and_now_is_the_one_handed_in() {
        assert_eq!(interval(None, None, 0).unwrap(), None);
        let i = interval(Some("now-1s"), None, 3_000_000_000)
            .unwrap()
            .unwrap();
        assert_eq!(i.start_time.unwrap().seconds, 2);
        assert!(interval(Some("yesterday"), None, 0).is_err());
    }
}
