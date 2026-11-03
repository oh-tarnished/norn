//! `FindEvents`: deterministic, domain-independent event detection.
//!
//! An event is one of exactly three things, so no threshold or judgement is hidden:
//! a sample passing the sample filter, or a sample whose value or quality differs
//! from the previous sample of its signal. It reads in signal order, which needs
//! only one previous sample in memory however many signals are selected. That
//! sample crosses calls in the page token, so a change at a page boundary is
//! detected exactly as one inside a page.

use std::sync::Arc;

use norn_core::{Sample, Value};
use norn_metadata::Registry;
use norn_protocol::query::{EventKind, FindEventsRequest, FindEventsResponse, SampleEvent};
use norn_runtime::query::{Engine, Order, Output, Position, QuerySpec};
use prost::Message;
use tonic::Status;

use super::pull::{Item, PAGE_BYTES, cannot, cut, each, open, page_size, value_bytes};
use super::request::{print_of, sample_filter, targets, time_range};
use super::token::{Previous, decode, encode};
use crate::convert::{quality_to_wire, sample_to_wire, value_to_wire};
use crate::status::bad_field;

/// The most samples one call reads. Past it, a call returns what it found with a
/// token, so a search of a quiet year cannot hold the daemon's query budget.
pub(crate) const MAX_SCANNED: u64 = 1_000_000;
const DEFAULT_PAGE: usize = 100;
const MAX_PAGE: usize = 1000;

/// What a change kind compares: the value's stored bytes, so equality is bit for
/// bit (-0.0 differs from 0.0, a NaN equals the same NaN, an enum is its number).
fn compared(kind: EventKind, s: &Sample) -> Result<Previous, Status> {
    Ok(match kind {
        EventKind::QualityChange => Previous::Quality(s.quality),
        _ => {
            let mut bytes = Vec::new();
            s.value.encode(&mut bytes).map_err(cannot)?;
            Previous::Value(bytes)
        }
    })
}

/// The event for `s`, which changed from `earlier`, if anything.
fn event(
    kind: EventKind,
    name: &str,
    s: &Sample,
    earlier: Option<&Previous>,
) -> Result<SampleEvent, Status> {
    let mut out = SampleEvent {
        kind: kind as i32,
        sample: Some(sample_to_wire(s, name.to_owned()).map_err(cannot)?),
        ..SampleEvent::default()
    };
    match earlier {
        Some(Previous::Value(bytes)) => {
            let (value, _) = Value::decode(bytes).map_err(cannot)?;
            out.previous_value = Some(value_to_wire(&value).map_err(cannot)?);
        }
        Some(Previous::Quality(q)) => out.previous_quality = Some(quality_to_wire(*q)),
        None => {}
    }
    Ok(out)
}

/// The kind asked for, checked against the rest of `request`.
fn kind_of(request: &FindEventsRequest) -> Result<EventKind, Status> {
    match EventKind::try_from(request.kind) {
        Ok(EventKind::Match) if request.sample_filter.trim().is_empty() => Err(bad_field(
            "sample_filter",
            "EVENT_KIND_MATCH needs a predicate",
        )),
        Ok(EventKind::Unspecified) | Err(_) => Err(bad_field("kind", "is required")),
        Ok(kind) => Ok(kind),
    }
}

/// Answers `request`. Blocks on the disk and the query budget.
///
/// # Errors
/// `INVALID_ARGUMENT` for a malformed field or token; whatever the query fails with.
pub(super) fn find(
    registry: &Registry,
    engine: &Arc<Engine>,
    request: FindEventsRequest,
) -> Result<FindEventsResponse, Status> {
    let kind = kind_of(&request)?;
    let limit = page_size("page_size", request.page_size, DEFAULT_PAGE, MAX_PAGE)?;
    let longest = value_bytes(request.max_value_bytes)?;
    let mut bare = request.clone();
    bare.page_token.clear();
    bare.page_size = 0;
    bare.max_value_bytes = 0;
    let print = print_of(&bare);
    let resumed = decode(&request.page_token, print)?;
    let (resume, mut previous) = resumed.map_or((None, None), |(p, prev)| (Some(p), prev));
    let mut signal = resume.as_ref().map(|p| match p {
        Position::Sample { signal, .. } | Position::Window { signal, .. } => signal.clone(),
        _ => String::new(),
    });
    let spec = QuerySpec {
        targets: targets(registry, &request.parent, &request.filter)?,
        range: time_range(request.interval, "interval")?,
        filter: sample_filter(&request.sample_filter)?,
        output: Output::Samples(Order::BySignal),
        resume,
    };
    let mut query = open(engine, spec)?;
    let mut out = FindEventsResponse::default();
    let (mut bytes, mut last) = (0, None);
    let mut scanned = 0u64;
    let more = each(&mut query, |item| {
        let Item::Sample(name, s) = item else {
            return Ok(true);
        };
        let full = out.events.len() >= limit || (!out.events.is_empty() && bytes >= PAGE_BYTES);
        if scanned >= MAX_SCANNED || full {
            return Ok(false);
        }
        let now = (kind != EventKind::Match)
            .then(|| compared(kind, &s))
            .transpose()?;
        let same_signal = signal.as_deref() == Some(&*name);
        let earlier = previous.as_ref().filter(|_| same_signal);
        let changed = match (&now, earlier) {
            (None, _) => Some(None),
            (Some(n), Some(e)) if n != e => Some(Some(e)),
            _ => None,
        };
        if let Some(from) = changed {
            let mut found = event(kind, &name, &s, from)?;
            let value = found.sample.as_mut().and_then(|s| s.value.as_mut());
            found.value_truncated = cut(value, longest);
            found.value_truncated |= cut(found.previous_value.as_mut(), longest);
            bytes += found.encoded_len();
            out.events.push(found);
        }
        scanned += 1;
        signal = Some(name.to_string());
        previous = now;
        let (time, sequence) = (s.timestamp, s.sequence.get());
        let at = name.to_string();
        last = Some(Position::Sample {
            signal: at,
            time,
            sequence,
        });
        Ok(true)
    })?;
    out.scanned_sample_count = i64::try_from(scanned).unwrap_or(i64::MAX);
    if more && let Some(position) = last {
        out.next_page_token = encode(print, &position, previous.as_ref());
    }
    Ok(out)
}
