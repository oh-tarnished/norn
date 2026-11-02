//! find_events: deterministic, domain-independent event detection.
//!
//! An event is one of exactly three things, so no threshold or judgement is hidden:
//! a sample passing a predicate the daemon evaluates, or a sample whose value or
//! quality differs from the signal's previous sample. Reads in signal order, which
//! needs only the previous sample of one signal, so memory stays constant however
//! many signals are selected. That previous sample crosses calls in the continuation.

use norn_protocol::common::Sample;
use norn_protocol::query::{QuerySamplesRequest, SampleOrder};
use serde_json::{Value, json};

use super::args_more::{EventKind, FindEvents};
use super::parse;
use crate::cursor::{Order, Previous};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::page::{DEFAULT_LIMIT, MAX_LIMIT, Rows, start};
use crate::render;
use crate::select::{interval, selection};
use crate::walk::{Item, walk};

/// The most samples one call reads. Past it, a call returns what it has, with a
/// continuation, so a search of a quiet year cannot hold the daemon's budget.
const MAX_SCANNED: u64 = 1_000_000;

/// What `kind` compares of a sample: its hash, and what a row shows.
fn compared(kind: EventKind, s: &Sample) -> Previous {
    if kind == EventKind::QualityChange {
        let shown = render::quality(s.quality.as_ref());
        return Previous {
            h: render::fnv(shown.as_bytes()),
            v: json!(shown),
        };
    }
    Previous {
        h: render::bits(s.value.as_ref()),
        v: render::value(s.value.as_ref()).0,
    }
}

/// The state of one search.
struct Search<'a> {
    kind: EventKind,
    rows: &'a mut Rows,
    signal: String,
    previous: Option<Previous>,
    scanned: u64,
}

impl Search<'_> {
    /// Takes one sample; false leaves it for the continuation.
    fn visit(&mut self, s: &Sample) -> bool {
        if self.scanned >= MAX_SCANNED {
            return false;
        }
        let mut row = render::sample(s);
        render::set(&mut row, "event", json!(self.kind_name()));
        if self.kind == EventKind::Match {
            self.scanned += 1;
            return self.rows.push(row);
        }
        let now = compared(self.kind, s);
        let earlier = self.previous.as_ref().filter(|_| self.signal == s.signal);
        if let Some(earlier) = earlier.filter(|p| p.h != now.h) {
            render::set(&mut row, "previous", earlier.v.clone());
            if !self.rows.push(row) {
                return false;
            }
        }
        self.scanned += 1;
        self.signal.clone_from(&s.signal);
        self.previous = Some(now);
        true
    }

    fn kind_name(&self) -> &'static str {
        match self.kind {
            EventKind::Match => "match",
            EventKind::ValueChange => "value_change",
            EventKind::QualityChange => "quality_change",
        }
    }
}

/// The tool.
pub(super) async fn find_events(call: &Call, raw: &Value) -> Result<Value, ToolError> {
    let args: FindEvents = parse(raw)?;
    if args.kind == EventKind::Match && args.sample_filter.is_none() {
        return Err(ToolError::invalid(
            "where",
            "kind `match` needs a predicate",
        ));
    }
    let cont = args.continuation.as_deref();
    let begin = start(call, raw, args.limit, DEFAULT_LIMIT, MAX_LIMIT, cont)?;
    let chosen = selection(&args.target, args.select.as_deref())?;
    let request = QuerySamplesRequest {
        parent: chosen.parent,
        filter: chosen.filter,
        interval: interval(args.from.as_deref(), args.to.as_deref(), begin.now)?,
        sample_filter: args.sample_filter.clone().unwrap_or_default(),
        order: SampleOrder::BySignal as i32,
        ..QuerySamplesRequest::default()
    };
    let cursor = begin.cursor;
    let mut rows = Rows::new(begin.limit);
    let mut search = Search {
        kind: args.kind,
        rows: &mut rows,
        signal: cursor.a.as_ref().map(|a| a.n.clone()).unwrap_or_default(),
        previous: cursor.p.clone(),
        scanned: 0,
    };
    let next = walk(call, request, Order::Signal, &cursor, |item| match item {
        Item::Sample(s) => search.visit(&s),
        Item::Aggregate(_) => true,
    })
    .await?;
    let (scanned, previous) = (search.scanned, search.previous);
    let next = next.map(|mut c| {
        c.p = previous;
        c
    });
    let mut result = rows.finish(next);
    render::set(&mut result, "samples_scanned", json!(scanned));
    Ok(result)
}
