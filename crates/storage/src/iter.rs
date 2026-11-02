//! Streaming samples out of a segment, one decoded chunk at a time.

use norn_core::{Sample, Timestamp};

use crate::chunk::Chunk;
use crate::error::Result;
use crate::format::frame::ChunkMeta;
use crate::segment::SegmentReader;

/// An iterator over the samples of selected chunks whose timestamps fall in
/// `from..=to`, in chunk order, then append order within a chunk.
///
/// Memory is bounded by one decoded chunk (at most `max_chunk_samples`
/// samples) plus the list of selected chunk metadata: a year of history
/// streams through the same footprint as an hour.
///
/// Failure: a chunk that fails to read or decode yields one `Err` item,
/// then iteration continues with the next chunk, so one corrupt chunk never
/// hides its neighbours (SPEC §20). Callers that want all-or-nothing stop
/// at the first `Err`.
///
/// Concurrency: borrows the reader mutably for its lifetime.
#[derive(Debug)]
pub struct SampleIter<'r> {
    reader: &'r mut SegmentReader,
    metas: std::vec::IntoIter<ChunkMeta>,
    current: Option<Chunk>,
    pos: usize,
    from: Timestamp,
    to: Timestamp,
}

impl<'r> SampleIter<'r> {
    pub(crate) fn new(
        reader: &'r mut SegmentReader,
        metas: Vec<ChunkMeta>,
        from: Timestamp,
        to: Timestamp,
    ) -> Self {
        Self {
            reader,
            metas: metas.into_iter(),
            current: None,
            pos: 0,
            from,
            to,
        }
    }
}

impl Iterator for SampleIter<'_> {
    type Item = Result<Sample>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(chunk) = &self.current {
                while let Some(s) = chunk.sample(self.pos) {
                    self.pos += 1;
                    if s.timestamp >= self.from && s.timestamp <= self.to {
                        return Some(Ok(s));
                    }
                }
                self.current = None;
            }
            let meta = self.metas.next()?;
            match self.reader.read_chunk(&meta) {
                Ok(chunk) => {
                    self.current = Some(chunk);
                    self.pos = 0;
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
