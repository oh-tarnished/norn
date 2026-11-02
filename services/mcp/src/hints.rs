//! What to try next, for each failure a model can do something about.

use std::collections::BTreeMap;

use crate::time;

/// A `min_time` or `max_time` from `ErrorInfo`, Unix nanoseconds, as RFC 3339.
fn instant(metadata: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let nanos = metadata.get(key)?.parse::<i128>().ok()?;
    time::timestamp(nanos).map(time::format)
}

/// The hint for `reason`, if it has one. Pure, so each is testable alone.
pub(crate) fn hint(reason: &str, metadata: &BTreeMap<String, String>) -> Option<String> {
    let text = match reason {
        "DAEMON_UNAVAILABLE" => "The Norn daemon is not answering at the endpoint this server was \
                                 started with. Check it is running (`nornctl status`), then retry."
            .to_owned(),
        "ERROR_REASON_DATA_CORRUPTED" => {
            let around = match (instant(metadata, "min_time"), instant(metadata, "max_time")) {
                (Some(a), Some(b)) => format!(
                    "Read around the damage: query intervals ending at {a} and starting after {b}"
                ),
                _ => "Read around the damage".to_owned(),
            };
            format!(
                "Stored history failed its checksum. Nothing was skipped, so nothing returned is \
                 wrong, but these samples cannot be read. {around}, or select other signals. \
                 Report the damaged chunk to an operator."
            )
        }
        "ERROR_REASON_INVALID_ARGUMENT" => {
            let argument = match metadata.get("field").map(String::as_str) {
                Some("sample_filter") => "`where`",
                Some("filter") => "`select` or `filter`",
                Some("interval" | "interval.start_time" | "interval.end_time") => "`from` or `to`",
                Some("state_time") => "`at`",
                Some("parent" | "scope") => "`target`",
                _ => "the arguments",
            };
            format!("The daemon refused {argument}; correct it and call again.")
        }
        "ERROR_REASON_INVALID_TOKEN" => "Repeat the call without `continuation`.".to_owned(),
        "ERROR_REASON_CAPACITY_EXHAUSTED" | "ERROR_REASON_LIMIT_EXCEEDED" => {
            "A bound of the daemon was reached. Narrow the request, or retry shortly.".to_owned()
        }
        "ERROR_REASON_RESOURCE_NOT_FOUND" | "ERROR_REASON_SIGNAL_NOT_FOUND" => {
            "Use list_entities and list_signals to find the names that exist.".to_owned()
        }
        "ERROR_REASON_REPLAY_STATE_CONFLICT" => {
            "Read the session with action `status`. An ended session accepts `seek` or `stop`."
                .to_owned()
        }
        "ERROR_REASON_SHUTTING_DOWN" => "The daemon is stopping. Retry once it is back.".to_owned(),
        "ERROR_REASON_STORAGE_UNAVAILABLE" => {
            "Norn's files cannot be read. This needs an operator.".to_owned()
        }
        _ => return None,
    };
    Some(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_damaged_chunk_says_which_interval_to_read_around() {
        let metadata = BTreeMap::from([
            ("min_time".to_owned(), "1000000000000".to_owned()),
            ("max_time".to_owned(), "1049000000000".to_owned()),
        ]);
        let text = hint("ERROR_REASON_DATA_CORRUPTED", &metadata).unwrap();
        assert!(text.contains("1970-01-01T00:16:40Z"), "{text}");
        assert!(text.contains("1970-01-01T00:17:29Z"), "{text}");
    }

    #[test]
    fn a_refused_field_is_named_as_the_tool_argument_a_model_typed() {
        let metadata = BTreeMap::from([("field".to_owned(), "sample_filter".to_owned())]);
        let text = hint("ERROR_REASON_INVALID_ARGUMENT", &metadata).unwrap();
        assert!(text.contains("`where`"), "{text}");
    }

    #[test]
    fn a_reason_with_no_advice_has_no_hint() {
        assert_eq!(hint("ERROR_REASON_SOMETHING_NEW", &BTreeMap::new()), None);
    }
}
