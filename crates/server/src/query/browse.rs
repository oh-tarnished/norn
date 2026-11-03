//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `BrowseSamples`: one bounded page of what `QuerySamples` streams.
//!
//! The query is the stream's, resumed from the page token. The page stops after
//! `page_size` results or [`PAGE_BYTES`], and its token names the last result
//! returned, so the next call resumes inside an engine page exactly: nothing is
//! lost or repeated, and a sample arriving between calls cannot shift a page.

use std::sync::Arc;

use norn_core::Timestamp;
use norn_metadata::Registry;
use norn_protocol::query::{BrowseSamplesRequest, BrowseSamplesResponse, QuerySamplesResponse};
use norn_runtime::query::{Aggregate, Engine, Function, Output, Position, QuerySpec, TimeRange};
use prost::Message;
use tonic::Status;

use super::pull::{Item, PAGE_BYTES, cannot, cut, each, open, page_size, value_bytes};
use super::request::{output, position, print_of, sample_filter, targets, time_range, token};
use crate::convert::{sample_to_wire, time_to_wire, value_to_wire};

/// The default and largest `page_size`.
const DEFAULT_PAGE: usize = 100;
const MAX_PAGE: usize = 1000;

/// Where an aggregate lies, as a resume position: its window's index from the
/// query's origin, the interval's start or the Unix epoch, as norn-query counts.
fn window_position(a: &Aggregate, output: Output, range: TimeRange) -> Position {
    let index = match output {
        Output::Aggregates(r) if r.function != Function::Latest => r.window.map_or(0, |w| {
            let origin = range.start.map_or(0, Timestamp::as_nanos);
            let start = a.start.map_or(origin, Timestamp::as_nanos);
            let k = (i128::from(start) - i128::from(origin)).div_euclid(i128::from(w));
            i64::try_from(k).unwrap_or(i64::MAX)
        }),
        _ => 0,
    };
    Position::Window {
        signal: a.signal.to_string(),
        index,
    }
}

/// Adds `item` to `out`; returns its position and encoded size.
fn add(
    out: &mut QuerySamplesResponse,
    item: Item,
    shape: (Output, TimeRange),
) -> Result<(Position, usize), Status> {
    Ok(match item {
        Item::Sample(name, s) => {
            let position = Position::Sample {
                signal: name.to_string(),
                time: s.timestamp,
                sequence: s.sequence.get(),
            };
            let wire = sample_to_wire(&s, name.to_string()).map_err(cannot)?;
            let size = wire.encoded_len();
            out.samples.push(wire);
            (position, size)
        }
        Item::Aggregate(a) => {
            let position = window_position(&a, shape.0, shape.1);
            let wire = norn_protocol::query::Aggregate {
                signal: a.signal.to_string(),
                window: Some(norn_protocol::google::r#type::Interval {
                    start_time: a.start.map(time_to_wire),
                    end_time: a.end.map(time_to_wire),
                }),
                value: Some(value_to_wire(&a.value).map_err(cannot)?),
                sample_count: i64::try_from(a.samples).unwrap_or(i64::MAX),
            };
            let size = wire.encoded_len();
            out.aggregates.push(wire);
            (position, size)
        }
    })
}

/// Answers `request`. Blocks on the disk and the query budget.
///
/// # Errors
/// `INVALID_ARGUMENT` for a malformed field or token; whatever the query fails with.
pub(super) fn browse(
    registry: &Registry,
    engine: &Arc<Engine>,
    request: BrowseSamplesRequest,
) -> Result<BrowseSamplesResponse, Status> {
    let limit = page_size("page_size", request.page_size, DEFAULT_PAGE, MAX_PAGE)?;
    let longest = value_bytes(request.max_value_bytes)?;
    let mut bare = request.clone();
    bare.page_token.clear();
    bare.page_size = 0;
    bare.max_value_bytes = 0;
    let print = print_of(&bare);
    let shape = (
        output(request.aggregation, request.order)?,
        time_range(request.interval, "interval")?,
    );
    let spec = QuerySpec {
        targets: targets(registry, &request.parent, &request.filter)?,
        range: shape.1,
        filter: sample_filter(&request.sample_filter)?,
        output: shape.0,
        resume: position(&request.page_token, print)?,
    };
    let mut query = open(engine, spec)?;
    let (mut out, mut bytes, mut count) = (QuerySamplesResponse::default(), 0, 0);
    let mut last = None;
    let more = each(&mut query, |item| {
        if count >= limit || (count > 0 && bytes >= PAGE_BYTES) {
            return Ok(false);
        }
        let (position, size) = add(&mut out, item, shape)?;
        count += 1;
        bytes += size;
        last = Some(position);
        Ok(true)
    })?;
    let mut truncated = Vec::new();
    let values = out.samples.iter_mut().map(|s| s.value.as_mut());
    let values = values.chain(out.aggregates.iter_mut().map(|a| a.value.as_mut()));
    for (i, value) in values.enumerate() {
        if cut(value, longest) {
            truncated.push(i32::try_from(i).unwrap_or(i32::MAX));
        }
    }
    let next = last.filter(|_| more).map(|p| token(print, &p));
    Ok(BrowseSamplesResponse {
        samples: out.samples,
        aggregates: out.aggregates,
        next_page_token: next.unwrap_or_default(),
        truncated_indexes: truncated,
    })
}
