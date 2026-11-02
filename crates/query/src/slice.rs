//! Choosing how much of time to read at once.
//!
//! A read holds its rows until they are sorted, so it must not cover more time
//! than fits. How much fits depends on density, and chunks may overlap: 16,000
//! signals' hour-long chunks all start together, and a merged order needs them
//! all. Cutting by time handles every shape with one mechanism.

use norn_core::Timestamp;
use norn_index::{ChunkLocator, PlanRequest};

use crate::engine::Engine;
use crate::rows::ROW_BYTES;
use crate::selection::Selection;

/// How many times an estimate may shrink. Each at least halves the width, so this
/// is more than an `i64` of nanoseconds can need.
const MAX_SHRINKS: u32 = 64;

/// The width to try next, adapting to what the last read found.
///
/// Starts as everything left, so a small result is one read. Concurrency: owned
/// by one query.
#[derive(Debug)]
pub(crate) struct Slicer {
    width: i128,
}

/// Inclusive `end` as nanoseconds after `cursor` + `width` - 1, clipped to `to`.
fn end_of(cursor: Timestamp, width: i128, to: Timestamp) -> Timestamp {
    let end = i128::from(cursor.as_nanos()) + width - 1;
    Timestamp::from_nanos(i64::try_from(end.min(i128::from(to.as_nanos()))).unwrap_or(i64::MAX))
}

/// How many of `l`'s samples fall in `from..=to`, if they are evenly spread.
fn share(l: &ChunkLocator, from: Timestamp, to: Timestamp) -> f64 {
    let (min, max) = (
        i128::from(l.min_ts().as_nanos()),
        i128::from(l.max_ts().as_nanos()),
    );
    let low = min.max(i128::from(from.as_nanos()));
    let high = max.min(i128::from(to.as_nanos()));
    let span = max - min + 1;
    let inside = (high - low + 1).max(0);
    f64::from(l.chunk.count) * (inside as f64 / span as f64)
}

impl Slicer {
    pub(crate) fn new(from: Timestamp, to: Timestamp) -> Self {
        let width = i128::from(to.as_nanos()) - i128::from(from.as_nanos()) + 1;
        Self { width }
    }

    /// The inclusive end of the next read from `cursor`, chosen so the chunks it
    /// touches should fit in `cap` bytes. An estimate from chunk metadata alone:
    /// it reads sidecars, never a segment, and a wrong guess only costs a
    /// [`Slicer::narrow`]. It cannot see the tail. A plan that stops early counts
    /// as too large: the read meets whatever stopped it, and handles it.
    pub(crate) fn end(
        &mut self,
        engine: &Engine,
        selection: &Selection,
        cursor: Timestamp,
        to: Timestamp,
        cap: u64,
    ) -> Timestamp {
        let most = (cap / ROW_BYTES) as f64;
        let snapshot = engine.index.snapshot();
        let mut end = end_of(cursor, self.width, to);
        for _ in 0..MAX_SHRINKS {
            let request = PlanRequest::new(cursor, end).signals(selection.filter.clone());
            let (mut held, mut over) = (0.0, None);
            let mut stopped = false;
            for locator in snapshot.plan(request) {
                let Ok(locator) = locator else {
                    stopped = true;
                    break;
                };
                held += share(&locator, cursor, end);
                if held > most {
                    over = Some(locator.min_ts());
                    break;
                }
            }
            let span = i128::from(end.as_nanos()) - i128::from(cursor.as_nanos());
            if (over.is_none() && !stopped) || span == 0 {
                break;
            }
            let half = i128::from(cursor.as_nanos()) + span / 2;
            let before = over.map_or(half, |t| i128::from(t.as_nanos()) - 1);
            let next = half.max(before).min(i128::from(end.as_nanos()) - 1);
            end = Timestamp::from_nanos(i64::try_from(next).unwrap_or(i64::MAX));
        }
        self.width = i128::from(end.as_nanos()) - i128::from(cursor.as_nanos()) + 1;
        end
    }

    /// A read of `cursor..=end` did not fit: try half.
    pub(crate) fn narrow(&mut self, cursor: Timestamp, end: Timestamp) {
        let span = i128::from(end.as_nanos()) - i128::from(cursor.as_nanos()) + 1;
        self.width = (span / 2).max(1);
    }

    /// A read fitted: the next may cover twice the time, if there was room.
    pub(crate) fn widen(&mut self, used: u64, cap: u64) {
        if used < cap / 2 {
            self.width = self.width.saturating_mul(2);
        }
    }
}
