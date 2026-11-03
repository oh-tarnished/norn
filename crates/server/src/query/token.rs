//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `FindEvents` page tokens: where the search stopped, and what it compares the
//! next sample of that signal with.
//!
//! `e1.<fingerprint>.<previous>.<position>`, where `<previous>` is empty for
//! `EVENT_KIND_MATCH`, `v` and the value's stored bytes in hex, or `q` and the
//! quality's two wire numbers. Opaque to a caller, not secret: the fingerprint
//! binds it to the request it came from, so it cannot resume another search.

use norn_core::Quality;
use norn_protocol::common::ErrorReason;
use norn_protocol::common::Quality as WireQuality;
use norn_runtime::query::Position;
use tonic::{Code, Status};

use crate::convert::{quality_from_wire, quality_to_wire};
use crate::status::error_info;

const PREFIX: &str = "e1";

/// The last sample read's comparable part.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Previous {
    /// Its value's stored bytes, which compare bit for bit.
    Value(Vec<u8>),
    /// Its quality.
    Quality(Quality),
}

/// The token resuming after `position`, with `previous` to compare with.
pub(crate) fn encode(print: u64, position: &Position, previous: Option<&Previous>) -> String {
    let previous = match previous {
        None => String::new(),
        Some(Previous::Value(bytes)) => {
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            format!("v{hex}")
        }
        Some(Previous::Quality(q)) => {
            let w = quality_to_wire(*q);
            format!("q{}-{}", w.severity, w.substatus)
        }
    };
    format!("{PREFIX}.{print:016x}.{previous}.{}", position.encode())
}

fn hex(text: &str) -> Option<Vec<u8>> {
    if !text.len().is_multiple_of(2) {
        return None;
    }
    let pairs = text.as_bytes().chunks(2);
    pairs
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).ok()?, 16).ok())
        .collect()
}

fn previous(text: &str) -> Option<Option<Previous>> {
    if text.is_empty() {
        return Some(None);
    }
    if let Some(bytes) = text.strip_prefix('v') {
        return Some(Some(Previous::Value(hex(bytes)?)));
    }
    let (severity, substatus) = text.strip_prefix('q')?.split_once('-')?;
    let wire = WireQuality {
        severity: severity.parse().ok()?,
        substatus: substatus.parse().ok()?,
    };
    Some(Some(Previous::Quality(quality_from_wire(wire).ok()?)))
}

/// Reads a token of a search of `print`; `None` for the empty token.
///
/// # Errors
/// `INVALID_ARGUMENT` with `ERROR_REASON_INVALID_TOKEN` for anything else.
pub(crate) fn decode(
    text: &str,
    print: u64,
) -> Result<Option<(Position, Option<Previous>)>, Status> {
    if text.is_empty() {
        return Ok(None);
    }
    let parsed = text
        .strip_prefix(&format!("{PREFIX}.{print:016x}."))
        .and_then(|rest| rest.split_once('.'))
        .and_then(|(prev, at)| Some((Position::parse(at)?, previous(prev)?)));
    let bad = || {
        let message = "the page token does not match this request";
        error_info(
            Code::InvalidArgument,
            ErrorReason::InvalidToken,
            message,
            [],
        )
    };
    parsed.map(Some).ok_or_else(bad)
}

#[cfg(test)]
mod tests {
    use super::*;
    use norn_core::Timestamp;

    fn at() -> Position {
        Position::Sample {
            signal: "entities/a/signals/b".to_owned(),
            time: Timestamp::from_nanos(-5),
            sequence: 7,
        }
    }

    #[test]
    fn a_token_round_trips_with_each_kind_of_previous() {
        let kinds = [
            None,
            Some(Previous::Value(vec![0, 0xff, 9])),
            Some(Previous::Quality(Quality::GOOD)),
        ];
        for previous in kinds {
            let text = encode(42, &at(), previous.as_ref());
            assert_eq!(decode(&text, 42).ok().flatten(), Some((at(), previous)));
        }
    }

    #[test]
    fn a_token_from_another_request_is_refused() {
        let text = encode(42, &at(), None);
        assert!(decode(&text, 43).is_err());
        assert!(decode("e1.garbage", 42).is_err());
    }
}
