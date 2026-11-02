//! Bounded results. Every list a tool returns is built here, so the bounds are in
//! one place: at most `limit` rows and [`MAX_RESULT_BYTES`] of JSON, with a
//! continuation whenever anything remains.

use serde_json::{Value, json};

use crate::cursor::{Cursor, fingerprint};
use crate::daemon::Call;
use crate::error::ToolError;

/// The default `limit` of a tool that lists rows.
pub(crate) const DEFAULT_LIMIT: u32 = 100;
/// The largest `limit`.
pub(crate) const MAX_LIMIT: u32 = 1_000;
/// The most JSON one result's rows may hold. Picked, not measured: a model's context
/// is the real bound, and this is several times what one should want at once.
pub(crate) const MAX_RESULT_BYTES: usize = 256 * 1_024;

/// Where one paged call starts.
#[derive(Debug)]
pub(crate) struct Start {
    pub(crate) cursor: Cursor,
    pub(crate) limit: usize,
    /// What `now` means for this read: the first page's, across all of them.
    pub(crate) now: i128,
}

/// Checks `limit` against `max`, and reads `continuation`.
pub(crate) fn start(
    call: &Call,
    raw: &Value,
    limit: Option<u32>,
    default_limit: u32,
    max: u32,
    continuation: Option<&str>,
) -> Result<Start, ToolError> {
    let limit = limit.unwrap_or(default_limit);
    if limit == 0 {
        return Err(ToolError::invalid("limit", "must be at least 1"));
    }
    if limit > max {
        let hint = "Lower `limit`, and follow `continuation` for the rest.";
        return Err(ToolError::limit("maximum limit", max as usize, hint));
    }
    let f = fingerprint(raw);
    let cursor = match continuation.filter(|c| !c.is_empty()) {
        Some(text) => Cursor::decode(text, f)?,
        None => Cursor {
            f,
            n: call.now.to_string(),
            ..Cursor::default()
        },
    };
    let now = cursor
        .n
        .parse()
        .map_err(|_| ToolError::continuation("this continuation is malformed"))?;
    Ok(Start {
        cursor,
        limit: limit as usize,
        now,
    })
}

/// A result's rows as they accumulate. Memory is bounded by the limits above.
#[derive(Debug)]
pub(crate) struct Rows {
    rows: Vec<Value>,
    limit: usize,
    bytes: usize,
}

impl Rows {
    pub(crate) fn new(limit: usize) -> Self {
        Self {
            rows: Vec::new(),
            limit,
            bytes: 0,
        }
    }

    /// Takes `row`, or says it does not fit. The first always fits, so a call always
    /// moves forward; [`crate::render`] bounds how large a row of samples can be.
    pub(crate) fn push(&mut self, row: Value) -> bool {
        let size = row.to_string().len();
        let full = self.rows.len() >= self.limit || self.bytes + size > MAX_RESULT_BYTES;
        if full && !self.rows.is_empty() {
            return false;
        }
        self.bytes += size;
        self.rows.push(row);
        true
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// The rows themselves.
    pub(crate) fn into_rows(self) -> Vec<Value> {
        self.rows
    }

    /// The result: `complete` is false exactly when `next` is the rest.
    pub(crate) fn finish(self, next: Option<Cursor>) -> Value {
        json!({
            "count": self.rows.len(),
            "rows": self.rows,
            "complete": next.is_none(),
            "continuation": next.map(|c| c.encode()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_stop_at_the_limit_and_at_the_byte_bound_and_the_first_always_fits() {
        let mut rows = Rows::new(2);
        assert!(rows.push(json!(1)) && rows.push(json!(2)));
        assert!(!rows.push(json!(3)));
        let text = "x".repeat(MAX_RESULT_BYTES);
        let mut rows = Rows::new(10);
        assert!(rows.push(json!(text)));
        assert!(!rows.push(json!(1)));
    }

    #[test]
    fn a_result_says_whether_more_remains() {
        let done = Rows::new(1).finish(None);
        assert_eq!(done["complete"], true);
        assert!(done["continuation"].is_null());
        let more = Rows::new(1).finish(Some(Cursor::default()));
        assert_eq!(more["complete"], false);
        assert!(more["continuation"].is_string());
    }
}
