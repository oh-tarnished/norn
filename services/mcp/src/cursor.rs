//! Continuations: how a bounded result resumes exactly where it stopped.
//!
//! The daemon resumes at its own page boundaries, and its pages can hold 10,000
//! samples, far past what one tool result may carry. So a continuation holds the
//! daemon's token for the page it stopped in, and the [`Key`] of the last row
//! returned: the next call reads that page again and drops what it has already
//! given. A key is a position and not a count, so a late sample arriving between
//! calls cannot shift it. The server keeps nothing between calls, so a
//! continuation outlives the MCP session and this process.

use std::cmp::Ordering;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::ToolError;

/// How the rows of a result are ordered, which is what "after a key" means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Order {
    /// (signal's full name, time, sequence): `SAMPLE_ORDER_BY_SIGNAL`, aggregates and
    /// state. The engine sorts by the whole name.
    Signal,
    /// (time, sequence, signal's full name): `SAMPLE_ORDER_BY_TIME`.
    Time,
    /// (entity, signal): the registry's listing order, which is not the whole name's,
    /// since `-` sorts below `/`.
    Listing,
}

/// A row's position.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Key {
    /// The signal's or the resource's full name.
    pub(crate) n: String,
    /// Nanoseconds since the epoch.
    pub(crate) t: i64,
    /// The sequence.
    pub(crate) s: i64,
}

fn listing(name: &str) -> (&str, &str) {
    name.split_once("/signals/").unwrap_or((name, ""))
}

impl Key {
    /// A resource, which has only a name.
    pub(crate) fn name(n: &str) -> Self {
        Self {
            n: n.to_owned(),
            t: 0,
            s: 0,
        }
    }

    /// How `self` compares with `other` in `order`.
    pub(crate) fn cmp_in(&self, other: &Self, order: Order) -> Ordering {
        match order {
            Order::Signal => (&self.n, self.t, self.s).cmp(&(&other.n, other.t, other.s)),
            Order::Time => (self.t, self.s, &self.n).cmp(&(other.t, other.s, &other.n)),
            Order::Listing => listing(&self.n).cmp(&listing(&other.n)),
        }
    }
}

/// What a change search last saw of the signal it stopped in, so a continuation
/// compares its first row with the row before it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Previous {
    /// A hash of the compared value's exact bits.
    pub(crate) h: u64,
    /// The value, as a result shows it.
    pub(crate) v: Value,
}

/// Where a read stands. The default is the start.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Cursor {
    /// A fingerprint of the arguments it was issued for.
    pub(crate) f: u64,
    /// What `now` meant on the first page, nanoseconds as text, so that `now-1h` names
    /// one interval across pages.
    pub(crate) n: String,
    /// The daemon's token for the page to read.
    pub(crate) r: String,
    /// The last row given, if any.
    pub(crate) a: Option<Key>,
    /// A change search's memory of that row.
    pub(crate) p: Option<Previous>,
}

impl Cursor {
    /// Whether a row at `key` was already given.
    pub(crate) fn given(&self, key: &Key, order: Order) -> bool {
        self.a
            .as_ref()
            .is_some_and(|a| key.cmp_in(a, order) != Ordering::Greater)
    }

    /// The text a tool result carries: hex, because the model must copy it exactly
    /// and an opaque token should look like one.
    pub(crate) fn encode(&self) -> String {
        let json = serde_json::to_vec(self).unwrap_or_default();
        json.iter().map(|b| format!("{b:02x}")).collect()
    }

    /// The cursor a result's text named. It carries the arguments' fingerprint, so
    /// using one with other arguments fails here rather than reading wrongly.
    pub(crate) fn decode(text: &str, fingerprint: u64) -> Result<Self, ToolError> {
        let bad = || ToolError::continuation("this continuation is malformed");
        let pairs = text.as_bytes().chunks(2);
        let hex = |p: &[u8]| {
            let p = std::str::from_utf8(p).ok()?;
            (p.len() == 2).then(|| u8::from_str_radix(p, 16).ok())?
        };
        let bytes: Option<Vec<u8>> = pairs.map(hex).collect();
        let cursor: Self = serde_json::from_slice(&bytes.ok_or_else(bad)?).map_err(|_| bad())?;
        if cursor.f != fingerprint {
            return Err(ToolError::continuation(
                "this continuation was issued for different arguments; only `limit` may change",
            ));
        }
        Ok(cursor)
    }
}

/// FNV-1a of `args`, all but `limit` and `continuation`: a fingerprint and not a
/// secret. Keys are sorted, so a client's ordering of them does not matter.
pub(crate) fn fingerprint(args: &Value) -> u64 {
    let mut entries: Vec<String> = Vec::new();
    if let Value::Object(map) = args {
        for (k, v) in map {
            if k != "limit" && k != "continuation" && !v.is_null() {
                entries.push(format!("{k}={v}"));
            }
        }
    }
    entries.sort_unstable();
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for byte in entries.join("\u{1}").bytes() {
        hash = (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn a_continuation_round_trips_and_is_bound_to_its_arguments() {
        let f = fingerprint(&json!({"target": "all", "limit": 5, "continuation": "x"}));
        assert_eq!(f, fingerprint(&json!({"limit": 9, "target": "all"})));
        assert_ne!(f, fingerprint(&json!({"target": "entities/a"})));
        let cursor = Cursor {
            f,
            r: "token".to_owned(),
            a: Some(Key::name("entities/a")),
            ..Cursor::default()
        };
        assert_eq!(Cursor::decode(&cursor.encode(), f).unwrap(), cursor);
        assert!(Cursor::decode(&cursor.encode(), f + 1).is_err());
        assert!(Cursor::decode("not hex", f).is_err());
        assert!(Cursor::decode("abc", f).is_err());
    }

    #[test]
    fn listing_order_is_by_entity_then_signal_and_not_by_the_whole_name() {
        let (a, b) = (
            Key::name("entities/a/signals/z"),
            Key::name("entities/a-b/signals/a"),
        );
        assert_eq!(a.cmp_in(&b, Order::Signal), Ordering::Greater);
        assert_eq!(a.cmp_in(&b, Order::Listing), Ordering::Less);
        let (a, b) = (Key::name("entities/a/signals/z"), Key::name("entities/ab"));
        assert_eq!(a.cmp_in(&b, Order::Listing), Ordering::Less);
    }
}
