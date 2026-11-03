//! What the unary reads share: their bounds, and pulling a query's results one at a
//! time on the calling (blocking) thread.

use std::sync::Arc;

use norn_core::Sample;
use norn_protocol::common::{Value, value::Kind};
use norn_runtime::query::{Aggregate, Engine, Query, QuerySpec, Results};
use tonic::Status;

use super::reply;
use crate::status::bad_field;

/// The encoded bytes after which a unary answer stops adding results. Its first
/// result always goes in, so a call always moves forward. Picked, not measured: a
/// page an AI agent can read whole, well under gRPC's 4 MiB message limit.
pub(crate) const PAGE_BYTES: usize = 256 << 10;

/// The longest string or bytes value returned whole when the caller names none.
const DEFAULT_VALUE_BYTES: usize = 1024;
/// The most a caller may ask a value to be before it is cut.
const MAX_VALUE_BYTES: usize = 64 << 10;

/// One result of a query.
pub(crate) enum Item {
    /// A raw sample, with its signal's resource name.
    Sample(Arc<str>, Sample),
    /// An aggregate.
    Aggregate(Aggregate),
}

/// The page size `asked` for, `default` when zero, at most `max`.
///
/// # Errors
/// `INVALID_ARGUMENT` naming `field` when negative.
pub(crate) fn page_size(
    field: &str,
    asked: i32,
    default: usize,
    max: usize,
) -> Result<usize, Status> {
    match usize::try_from(asked) {
        Ok(0) => Ok(default),
        Ok(n) => Ok(n.min(max)),
        Err(_) => Err(bad_field(field, "must not be negative")),
    }
}

/// The value length `asked` for in `max_value_bytes`: 1024 when zero, at most 64 KiB.
///
/// # Errors
/// `INVALID_ARGUMENT` when negative.
pub(crate) fn value_bytes(asked: i32) -> Result<usize, Status> {
    page_size(
        "max_value_bytes",
        asked,
        DEFAULT_VALUE_BYTES,
        MAX_VALUE_BYTES,
    )
}

/// Cuts a string or bytes `value` longer than `limit` bytes, a string at the last
/// character boundary at or below it. Returns whether it cut anything.
pub(crate) fn cut(value: Option<&mut Value>, limit: usize) -> bool {
    match value.and_then(|v| v.kind.as_mut()) {
        Some(Kind::StringValue(s)) if s.len() > limit => {
            s.truncate(s.floor_char_boundary(limit));
            true
        }
        Some(Kind::BytesValue(b)) if b.len() > limit => {
            b.truncate(limit);
            true
        }
        _ => false,
    }
}

/// Opens `spec` on `engine`. Blocks on the query budget.
///
/// # Errors
/// The query's refusal, as [`reply::status`] maps it.
pub(crate) fn open(engine: &Arc<Engine>, spec: QuerySpec) -> Result<Query, Status> {
    engine.open(spec).map_err(|e| reply::status(&e))
}

/// Feeds every result of `query` to `visit`, in order, until it returns false or
/// the results end. Blocks on the disk. Memory: one engine page at a time.
///
/// Returns true when `visit` stopped it, false when the results ran out.
///
/// # Errors
/// What the query fails with, such as `DATA_LOSS`, or what `visit` returns.
pub(crate) fn each(
    query: &mut Query,
    mut visit: impl FnMut(Item) -> Result<bool, Status>,
) -> Result<bool, Status> {
    while let Some(page) = query.next_page().map_err(|e| reply::status(&e))? {
        let items: Box<dyn Iterator<Item = Item>> = match page.results {
            Results::Samples(list) => Box::new(list.into_iter().map(|(n, s)| Item::Sample(n, s))),
            Results::Aggregates(list) => Box::new(list.into_iter().map(Item::Aggregate)),
            _ => return Err(Status::internal("a result of an unknown kind")),
        };
        for item in items {
            if !visit(item)? {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// A stored value the wire cannot carry, as the status that ends the call.
pub(crate) fn cannot(e: impl std::fmt::Display) -> Status {
    Status::internal(format!("cannot send a stored value: {e}"))
}
