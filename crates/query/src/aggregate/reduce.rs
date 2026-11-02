//! Feeding a query's rows through its windows.

use std::sync::Arc;

use norn_core::Value;

use super::{Accum, Aggregate, Windows};
use crate::rows::Row;
use crate::spec::{Function, Target};

/// The window being filled, for one signal at a time.
///
/// Rows arrive grouped by signal and in time order, so one window of one signal
/// is all that is ever open. Memory: one [`Accum`]. Concurrency: owned by one
/// query.
#[derive(Debug)]
pub(crate) struct Reduce {
    function: Function,
    windows: Windows,
    open: Option<(u32, i64, Accum)>,
}

impl Reduce {
    pub(crate) fn new(function: Function, windows: Windows) -> Self {
        Self {
            function,
            windows,
            open: None,
        }
    }

    /// Feeds `row`. Returns the window it closed, if it closed one with a result.
    pub(crate) fn add(
        &mut self,
        row: &Row,
        targets: &[(u32, Arc<Target>)],
    ) -> Option<(i64, Aggregate)> {
        let index = self.windows.index_of(row.sample.timestamp);
        let same = |o: &(u32, i64, Accum)| o.0 == row.target && o.1 == index;
        let closed = match self.open.take_if(|o| !same(o)) {
            Some(open) => self.close(open, targets),
            None => None,
        };
        let (_, _, accum) = self
            .open
            .get_or_insert_with(|| (row.target, index, Accum::new(self.function)));
        accum.add(row.sample.timestamp, &row.sample.value);
        closed
    }

    /// Closes what is open, at the end of the rows.
    pub(crate) fn finish(&mut self, targets: &[(u32, Arc<Target>)]) -> Option<(i64, Aggregate)> {
        let open = self.open.take()?;
        self.close(open, targets)
    }

    fn close(
        &self,
        (target, index, accum): (u32, i64, Accum),
        targets: &[(u32, Arc<Target>)],
    ) -> Option<(i64, Aggregate)> {
        let samples = accum.count();
        let value: Value = accum.finish()?;
        let signal = targets.get(target as usize)?.1.name.clone();
        let (start, end) = self.windows.bounds(index);
        let aggregate = Aggregate {
            signal,
            start,
            end,
            value,
            samples,
        };
        Some((index, aggregate))
    }
}
