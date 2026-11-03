//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How segments roll and how durable writes are grouped.

use std::time::Duration;

const MIB: u64 = 1024 * 1024;

/// Storage settings (SPEC §13, §19): when a segment rolls, and what stands
/// between a batch and `DURABLE`.
///
/// `#[non_exhaustive]`: start from [`Default`] and set fields.
///
/// `Default` (picked, not measured; stage 15 revisits them): segments roll at
/// 256 MiB or one hour of sample time, and are sealed after one hour open
/// whatever they hold; group commit waits at most 5 ms for company; a chunk's
/// samples wait in memory at most 60 s; durable acknowledgements are offered.
///
/// Invariant (checked by [`crate::ConfigBuilder::build`]): every size and
/// duration is non-zero. `durable_acks` is a choice, not a size: `false` means
/// the daemon has no durability boundary, offers no `CAPABILITY_ACK_DURABLE`,
/// and refuses a batch requesting `DURABLE` rather than acknowledge it at a
/// weaker level (docs/reference/protocol.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct StorageConfig {
    /// A segment rolls once its file reaches this many bytes.
    pub max_segment_bytes: u64,
    /// A segment rolls once its samples span this much sample time.
    pub max_segment_age: Duration,
    /// A segment is sealed once it has been open this long on the daemon's
    /// clock, however little it holds, so a quiet or short-lived stream still
    /// reaches sealed segments, which compaction, retention and the startup
    /// fast path work on. Measured on the injected clock by the maintenance
    /// task, not on sample times. Shorter means more, smaller segments, which
    /// compaction merges.
    pub max_segment_open_time: Duration,
    /// The longest a batch waiting for `DURABLE` waits for others to share
    /// its fsync. A longer interval means fewer fsyncs and higher latency.
    pub group_commit_interval: Duration,
    /// The longest a sample waits in its signal's buffer before it is encoded
    /// into a chunk. Bounds how stale the segments are, not durability: the
    /// journal holds the sample meanwhile (docs/reference/storage.md, "Write path").
    pub max_chunk_age: Duration,
    /// Whether the daemon offers `DURABLE`, that is, whether it fsyncs.
    pub durable_acks: bool,
}

impl StorageConfig {
    /// The name of the first setting that is zero, for validation.
    pub(crate) fn first_zero(&self) -> Option<&'static str> {
        if self.max_segment_bytes == 0 {
            Some("max_segment_bytes")
        } else if self.max_segment_age.is_zero() {
            Some("max_segment_age")
        } else if self.max_segment_open_time.is_zero() {
            Some("max_segment_open_time")
        } else if self.group_commit_interval.is_zero() {
            Some("group_commit_interval")
        } else if self.max_chunk_age.is_zero() {
            Some("max_chunk_age")
        } else {
            None
        }
    }
}

impl Default for StorageConfig {
    fn default() -> Self {
        Self {
            max_segment_bytes: 256 * MIB,
            max_segment_age: Duration::from_secs(3600),
            max_segment_open_time: Duration::from_secs(3600),
            group_commit_interval: Duration::from_millis(5),
            max_chunk_age: Duration::from_secs(60),
            durable_acks: true,
        }
    }
}
