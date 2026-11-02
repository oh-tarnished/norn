//! What every List and Update shares: pages, the AIP-160 subset, field masks.

use prost_types::FieldMask;
use tonic::{Code, Status};

use crate::status::{bad_field, error_info};
use norn_protocol::common::ErrorReason;

const DEFAULT_PAGE: usize = 100;
const MAX_PAGE: usize = 1000;
/// The longest filter parsed; a longer one is refused before any work.
const MAX_FILTER_BYTES: usize = 4096;

/// A page size within bounds.
pub(super) fn page_size(requested: i32) -> Result<usize, Status> {
    match usize::try_from(requested) {
        Ok(0) => Ok(DEFAULT_PAGE),
        Ok(n) => Ok(n.min(MAX_PAGE)),
        Err(_) => Err(bad_field("page_size", "must not be negative")),
    }
}

fn fingerprint(scope: &str, filter: &str) -> u64 {
    let bytes = scope.bytes().chain([0]).chain(filter.bytes());
    bytes.fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

/// A token resuming after resource `last`, bound to the request's `scope` (its
/// parent) and `filter`. Opaque to clients; not a secret or a snapshot.
pub(super) fn page_token(scope: &str, filter: &str, last: &str) -> String {
    format!("v1.{:016x}.{last}", fingerprint(scope, filter))
}

/// The resource a token resumes after; `None` for an empty one.
pub(super) fn after_token<'a>(
    token: &'a str,
    scope: &str,
    filter: &str,
) -> Result<Option<&'a str>, Status> {
    if token.is_empty() {
        return Ok(None);
    }
    let want = format!("v1.{:016x}.", fingerprint(scope, filter));
    token.strip_prefix(&want).map(Some).ok_or_else(|| {
        let text = "the page token does not match this request";
        error_info(Code::InvalidArgument, ErrorReason::InvalidToken, text, [])
    })
}

/// A `field = "value"` comparison.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Clause {
    pub(super) field: String,
    pub(super) value: String,
}

/// Parses the supported subset: clauses joined by `AND`, over `fields` or
/// `annotations.<key>` when `annotated`. Anything else is refused, never ignored.
pub(super) fn parse_filter(text: &str, fields: &[&str]) -> Result<Vec<Clause>, Status> {
    let refuse = |why: &str| bad_field("filter", why);
    if text.len() > MAX_FILTER_BYTES {
        return Err(refuse("is too long"));
    }
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    let mut clauses = Vec::new();
    for part in split_and(text) {
        let (field, value) = part
            .split_once('=')
            .ok_or_else(|| refuse("supports only field = \"value\" AND ..."))?;
        let field = field.trim();
        let value = value
            .trim()
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .filter(|v| !v.contains('"'))
            .ok_or_else(|| refuse("a value must be a double-quoted string"))?;
        let known = fields.contains(&field)
            || field
                .strip_prefix("annotations.")
                .is_some_and(|k| !k.is_empty());
        if !known {
            return Err(refuse("names a field that cannot be filtered on"));
        }
        clauses.push(Clause {
            field: field.to_owned(),
            value: value.to_owned(),
        });
    }
    Ok(clauses)
}

/// Splits on ` AND ` outside quotes.
fn split_and(text: &str) -> Vec<&str> {
    let (mut parts, mut start, mut quoted) = (Vec::new(), 0, false);
    for (at, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && text.get(at..).is_some_and(|t| t.starts_with(" AND ")) {
            parts.extend(text.get(start..at));
            start = at + 5;
        }
    }
    parts.extend(text.get(start..));
    parts
}

/// Whether every clause holds; `get` reads a field, an annotation by its dotted
/// name. An absent field equals nothing.
pub(super) fn matches(clauses: &[Clause], get: impl Fn(&str) -> Option<String>) -> bool {
    clauses
        .iter()
        .all(|c| get(&c.field).is_some_and(|v| v == c.value))
}

/// Which fields an Update changes.
#[derive(Debug)]
pub(super) enum Mask {
    /// No mask: whatever the request populated (AIP-134).
    Populated,
    /// `*`: every mutable field, an empty one clearing it.
    All,
    /// Exactly these.
    Paths(Vec<String>),
}

impl Mask {
    /// Reads `mask`, refusing a path that is immutable or unknown.
    pub(super) fn parse(mask: Option<FieldMask>, mutable: &[&str]) -> Result<Self, Status> {
        let paths = mask.map(|m| m.paths).unwrap_or_default();
        if paths.is_empty() {
            return Ok(Self::Populated);
        }
        if paths.iter().any(|p| p == "*") {
            return Ok(Self::All);
        }
        for path in &paths {
            if !mutable.contains(&path.as_str()) {
                return Err(bad_field("update_mask", "names a field that cannot change"));
            }
        }
        Ok(Self::Paths(paths))
    }

    /// Whether `field`, populated or not in the request, is to change.
    pub(super) fn wants(&self, field: &str, populated: bool) -> bool {
        match self {
            Self::Populated => populated,
            Self::All => true,
            Self::Paths(paths) => paths.iter().any(|p| p == field),
        }
    }
}
