//! Loading one source into a plan's heap.

use std::cmp::Reverse;

use super::scan::chunks_of;
use super::{Plan, Source};
use crate::error::{IndexError, Result};
use crate::locator::ChunkLocator;

impl Plan {
    /// Pushes `source`'s matching chunks, stopping at the buffer bound.
    ///
    /// # Errors
    /// [`IndexError::PlanTooLarge`]; [`IndexError::SegmentGone`].
    pub(super) fn load(&mut self, source: Source) -> Result<()> {
        let (from, to) = (self.request.from, self.request.to);
        let limit = self.snapshot.shared.options.max_plan_buffered;
        let heap = &mut self.heap;
        let mut over = false;
        let mut push = |l: ChunkLocator| {
            if heap.len() < limit {
                heap.push(Reverse(l));
            } else {
                over = true;
            }
        };
        match source {
            Source::Tail { index } => {
                if let Some((_, tail, stream)) = self.tails.get(index) {
                    let filter = &self.request.signals;
                    tail.for_each(from, to, |c| {
                        if filter.matches(c.signal) {
                            push(ChunkLocator {
                                stream: *stream,
                                segment: tail.segment,
                                sealed: false,
                                chunk: *c,
                            });
                        }
                    });
                }
                self.tails.truncate(index);
            }
            Source::Segment { cursor } => {
                let Some(c) = self.cursors.get_mut(cursor) else {
                    return Ok(());
                };
                let stream = c.view.stream;
                let at = c.window.start;
                c.window.start += 1;
                if let Some(record) = c.view.segments.records.get(at) {
                    let segment = record.name();
                    let filter = &self.request.signals;
                    let shared = &self.snapshot.shared;
                    chunks_of(shared, stream, record, filter, from, to, |c| {
                        push(ChunkLocator {
                            stream,
                            segment,
                            sealed: true,
                            chunk: *c,
                        });
                    })?;
                }
            }
        }
        if over {
            return Err(IndexError::PlanTooLarge {
                buffered: self.heap.len(),
                limit,
            });
        }
        Ok(())
    }
}
