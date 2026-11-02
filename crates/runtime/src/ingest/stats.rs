//! What a stream holds.

use norn_core::Timestamp;

/// What a stream holds. Exact: a sample counts from when it is in a buffer, which
/// is before its `PERSISTED` acknowledgement, and moves from `buffered` to stored
/// in one step, so `samples` never dips or doubles across a flush.
///
/// `corrupt_samples` are those in chunks known damaged: found by startup
/// verification, `VerifyStream`, compaction or a query. They are not in `samples`,
/// since a query cannot return them. Damage nothing has read yet is not known.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct StreamStats {
    /// Readable samples: stored, plus buffered.
    pub samples: u64,
    /// Of those, the samples not in a segment yet.
    pub buffered: u64,
    /// Samples in chunks known damaged.
    pub corrupt_samples: u64,
    /// Bytes on disk.
    pub stored_bytes: u64,
    /// The earliest and latest sample time, buffered samples included.
    pub time_range: Option<(Timestamp, Timestamp)>,
}

impl StreamStats {
    /// Widens these totals by `samples` already stored, of `bytes`, over `range`.
    pub(crate) fn add(&mut self, samples: u64, bytes: u64, range: Option<(Timestamp, Timestamp)>) {
        self.samples += samples;
        self.stored_bytes += bytes;
        self.widen(range);
    }

    /// Counts `samples` over `range` entering the buffers.
    pub(crate) fn buffer(&mut self, samples: u64, range: Option<(Timestamp, Timestamp)>) {
        self.samples += samples;
        self.buffered += samples;
        self.widen(range);
    }

    /// Moves `samples` of `bytes` from the buffers to disk.
    pub(crate) fn stored(&mut self, samples: u64, bytes: u64) {
        self.buffered = self.buffered.saturating_sub(samples);
        self.stored_bytes += bytes;
    }

    fn widen(&mut self, range: Option<(Timestamp, Timestamp)>) {
        self.time_range = match (self.time_range, range) {
            (Some((a, b)), Some((c, d))) => Some((a.min(c), b.max(d))),
            (a, b) => a.or(b),
        };
    }
}
