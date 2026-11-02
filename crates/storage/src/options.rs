//! Storage bounds and policies, derived from the engine [`Config`].

use norn_codec::MAX_BLOCK_VALUES;
use norn_codec::compress::Compression;
use norn_core::Config;

use crate::error::{Bound, Result, StorageError};
use crate::format::DIR_ENTRY_LEN;

const KIB: u64 = 1024;
const MIB: u64 = 1024 * KIB;

/// Every bound and policy the storage layer enforces (SPEC §16).
///
/// Derive it with [`StorageOptions::from_config`]; `Default` is
/// `from_config(&Config::default())`. Fields are public so a caller can
/// tighten one; `#[non_exhaustive]` so a new one is not a breaking change.
///
/// The roll limits come from `Config::storage()`; the memory-shaped bounds are
/// fractions of its budgets:
///
/// | Field | Default | Derived from |
/// | --- | --- | --- |
/// | `max_segment_bytes` | 256 MiB | `storage.max_segment_bytes` |
/// | `max_segment_span_nanos` | 1 hour | `storage.max_segment_age` |
/// | `max_chunks_per_segment` | 95,325 | `index_bytes / 16` of directory entries |
/// | `max_chunk_samples` | 86,400 | picked, at most `MAX_BLOCK_VALUES` |
/// | `max_chunk_bytes` | 16 MiB | `chunk_cache_bytes / 8`, at most 64 MiB |
/// | `write_buffer_bytes` | 1 MiB | `ingest_bytes / 64`, 64 KiB to 8 MiB |
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct StorageOptions {
    /// A segment rolls once its file reaches this many bytes.
    pub max_segment_bytes: u64,
    /// A segment rolls once its samples span this many nanoseconds, measured
    /// on sample timestamps (the storage layer reads no clock).
    pub max_segment_span_nanos: i64,
    /// Chunks in one segment; bounds the in-memory directory a writer keeps
    /// and a reader loads (`DIR_ENTRY_LEN` bytes each).
    pub max_chunks_per_segment: usize,
    /// Samples in one chunk; compaction merges up to this many.
    pub max_chunk_samples: usize,
    /// Bytes in one chunk frame body. A reader refuses a larger recorded
    /// length before allocating, so this bounds one chunk read.
    pub max_chunk_bytes: u32,
    /// Capacity of a segment writer's buffer.
    pub write_buffer_bytes: usize,
    /// How chunk columns are compressed on the write path.
    pub compression: Compression,
}

impl StorageOptions {
    /// Options sized from `config`'s memory budgets.
    #[must_use]
    pub fn from_config(config: &Config) -> Self {
        let m = config.memory();
        let s = config.storage();
        let dir_entries = (m.index_bytes / 16) / DIR_ENTRY_LEN as u64;
        let chunk_bytes = (m.chunk_cache_bytes / 8).clamp(KIB, 64 * MIB);
        let buffer = (m.ingest_bytes / 64).clamp(64 * KIB, 8 * MIB);
        Self {
            max_segment_bytes: s.max_segment_bytes,
            max_segment_span_nanos: i64::try_from(s.max_segment_age.as_nanos()).unwrap_or(i64::MAX),
            max_chunks_per_segment: usize::try_from(dir_entries.max(1)).unwrap_or(usize::MAX),
            max_chunk_samples: 86_400,
            max_chunk_bytes: u32::try_from(chunk_bytes).unwrap_or(u32::MAX),
            write_buffer_bytes: usize::try_from(buffer).unwrap_or(usize::MAX),
            compression: Compression::Default,
        }
    }

    /// Checks the options are usable: every bound non-zero and the chunk
    /// sample bound within the codec's block limit.
    ///
    /// # Errors
    /// [`StorageError::TooLarge`] for `max_chunk_samples` above
    /// `MAX_BLOCK_VALUES`; [`StorageError::InvalidChunk`] for a zero bound.
    pub fn validate(&self) -> Result<()> {
        if self.max_chunk_samples > MAX_BLOCK_VALUES {
            return Err(StorageError::TooLarge {
                bound: Bound::ChunkSamples,
                len: self.max_chunk_samples as u64,
                max: MAX_BLOCK_VALUES as u64,
            });
        }
        let zero = self.max_segment_bytes == 0
            || self.max_segment_span_nanos <= 0
            || self.max_chunks_per_segment == 0
            || self.max_chunk_samples == 0
            || self.max_chunk_bytes == 0
            || self.write_buffer_bytes == 0;
        if zero {
            return Err(StorageError::InvalidChunk("a storage bound is zero"));
        }
        Ok(())
    }
}

impl Default for StorageOptions {
    fn default() -> Self {
        Self::from_config(&Config::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_are_valid_and_follow_the_config_budgets() {
        let o = StorageOptions::default();
        assert!(o.validate().is_ok());
        assert_eq!(o.write_buffer_bytes, 1 << 20);
        assert_eq!(o.max_chunk_bytes, 16 << 20);
        assert_eq!(o.max_chunks_per_segment, (4 << 20) / DIR_ENTRY_LEN);
    }

    #[test]
    fn a_zero_bound_is_refused() {
        let o = StorageOptions {
            max_chunk_samples: 0,
            ..StorageOptions::default()
        };
        assert!(o.validate().is_err());
    }
}
