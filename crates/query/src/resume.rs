//! Continuing a query just after a position.

use std::sync::Arc;

use norn_core::Timestamp;

use crate::aggregate::Windows;
use crate::error::{QueryError, Result};
use crate::rows::Row;
use crate::spec::{Function, Order, Output, Target};
use crate::token::Position;

/// Skips rows at or before a resumed position.
#[derive(Debug, Clone, Copy)]
pub(crate) struct After {
    pub(crate) time: Timestamp,
    pub(crate) sequence: u64,
    /// The first target index that is not at or before the position's signal at
    /// equal time and sequence.
    pub(crate) next_target: u32,
    /// Rows of earlier targets at equal time and sequence were delivered too.
    pub(crate) merged: bool,
}

impl After {
    pub(crate) fn delivered(&self, row: &Row) -> bool {
        let at = (row.sample.timestamp, row.sample.sequence.get());
        let mine = (self.time, self.sequence);
        if self.merged {
            at < mine || (at == mine && row.target < self.next_target)
        } else {
            at <= mine
        }
    }
}

/// Where a walk begins.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Start {
    /// Skip this many targets.
    pub(crate) skip: usize,
    /// The first group begins here rather than at the range's start.
    pub(crate) from: Option<Timestamp>,
    /// Drop what was delivered.
    pub(crate) after: Option<After>,
}

/// Where a resumed query begins.
pub(crate) fn start_from(
    position: &Position,
    list: &[(u32, Arc<Target>)],
    output: Output,
    windows: Option<&Windows>,
) -> Result<Start> {
    let name = position.signal();
    let at = list.partition_point(|t| &*t.1.name < name);
    let exact = list.get(at).is_some_and(|t| &*t.1.name == name);
    let index_after = u32::try_from(at + usize::from(exact)).unwrap_or(u32::MAX);
    match (output, position) {
        (Output::Samples(Order::ByTime), Position::Sample { time, sequence, .. }) => Ok(Start {
            skip: 0,
            from: Some(*time),
            after: Some(After {
                time: *time,
                sequence: *sequence,
                next_target: index_after,
                merged: true,
            }),
        }),
        (Output::Samples(_), Position::Sample { time, sequence, .. }) => Ok(Start {
            skip: at,
            from: exact.then_some(*time),
            after: exact.then_some(After {
                time: *time,
                sequence: *sequence,
                next_target: index_after,
                merged: false,
            }),
        }),
        (Output::Aggregates(r), Position::Window { index, .. }) => {
            if r.function == Function::Latest || r.window.is_none() {
                return Ok(Start {
                    skip: at + usize::from(exact),
                    ..Start::default()
                });
            }
            let next = index.checked_add(1).ok_or(QueryError::BadPosition)?;
            let from = windows.and_then(|w| w.first_instant(next));
            Ok(Start {
                // Past the last representable window: this signal is done.
                skip: at + usize::from(exact && from.is_none()),
                from: from.filter(|_| exact),
                after: None,
            })
        }
        _ => Err(QueryError::BadPosition),
    }
}
