//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The segment header: identity and versions, checked before anything else.

use std::path::Path;

use norn_codec::CODEC_VERSION;
use norn_core::StreamId;

use super::{FORMAT_VERSION, HEADER_LEN, Le, SCHEMA_VERSION, SEGMENT_MAGIC};
use crate::crc32c::crc32c;
use crate::error::{CorruptionKind, Result, StorageError, VersionField};

/// The first [`HEADER_LEN`] bytes of a segment (SPEC §40).
///
/// | Offset | Size | Field |
/// | ---: | ---: | --- |
/// | 0 | 8 | magic `NORNSEG\0` |
/// | 8 | 2 | `format_version` |
/// | 10 | 2 | `schema_version` |
/// | 12 | 1 | `codec_version` |
/// | 13 | 1 | flags, zero |
/// | 14 | 2 | `generation`: 0 when written by ingest, +1 per compaction |
/// | 16 | 8 | stream id |
/// | 24 | 8 | `first_seq` |
/// | 32 | 8 | `last_seq`: equal to `first_seq` unless compaction merged segments |
/// | 40 | 4 | reserved, zero |
/// | 44 | 4 | CRC-32C of bytes 0..44 |
///
/// A segment with generation `g` covering `first_seq..=last_seq`
/// supersedes every segment of a lower generation whose sequence lies in
/// that range: that is how compaction switches atomically (docs/reference/storage.md).
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentHeader {
    /// The stream whose samples the segment holds.
    pub stream: StreamId,
    /// The lowest segment sequence number this segment covers.
    pub first_seq: u64,
    /// The highest segment sequence number this segment covers.
    pub last_seq: u64,
    /// How many compactions produced this segment.
    pub generation: u16,
    /// The codec layout version of its blocks.
    pub codec_version: u8,
}

impl SegmentHeader {
    /// A header for a fresh ingest segment, at this build's versions.
    #[must_use]
    pub fn new(stream: StreamId, seq: u64) -> Self {
        Self {
            stream,
            first_seq: seq,
            last_seq: seq,
            generation: 0,
            codec_version: CODEC_VERSION,
        }
    }

    /// The header's bytes, CRC included.
    #[must_use]
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let mut b = Vec::with_capacity(HEADER_LEN);
        b.extend_from_slice(&SEGMENT_MAGIC);
        b.extend_from_slice(&FORMAT_VERSION.to_le_bytes());
        b.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        b.extend([self.codec_version, 0]);
        b.extend_from_slice(&self.generation.to_le_bytes());
        b.extend_from_slice(&self.stream.get().to_le_bytes());
        b.extend_from_slice(&self.first_seq.to_le_bytes());
        b.extend_from_slice(&self.last_seq.to_le_bytes());
        b.extend_from_slice(&0u32.to_le_bytes());
        let crc = crc32c(&b);
        b.extend_from_slice(&crc.to_le_bytes());
        let mut out = [0u8; HEADER_LEN];
        out.copy_from_slice(&b);
        out
    }

    /// Parses and checks a header read from `path` (named in errors only).
    /// Copies; borrows nothing past the call.
    ///
    /// # Errors
    /// [`StorageError::Corrupt`] with [`CorruptionKind::TornTail`] for fewer
    /// than [`HEADER_LEN`] bytes, `BadMagic` or `HeaderChecksum`; and
    /// [`StorageError::UnsupportedVersion`] for any version newer than this
    /// build's (checked only once the CRC passes, so a bit flip is reported
    /// as corruption, not as a newer file).
    pub fn decode(bytes: &[u8], path: &Path) -> Result<Self> {
        let bad = |kind| StorageError::corrupt(path, 0, HEADER_LEN as u64, kind);
        let rec = bytes
            .get(..HEADER_LEN)
            .ok_or(bad(CorruptionKind::TornTail))?;
        let (body, crc) = rec.split_at(HEADER_LEN - 4);
        let mut r = Le::new(rec);
        if r.bytes::<8>() != Some(SEGMENT_MAGIC) {
            return Err(bad(CorruptionKind::BadMagic));
        }
        if crc != crc32c(body).to_le_bytes() {
            return Err(bad(CorruptionKind::HeaderChecksum));
        }
        let malformed = || bad(CorruptionKind::Malformed("segment header"));
        let format = r.u16().ok_or_else(malformed)?;
        let schema = r.u16().ok_or_else(malformed)?;
        let codec = r.u8().ok_or_else(malformed)?;
        let _flags = r.u8();
        let generation = r.u16().ok_or_else(malformed)?;
        let stream = r.u64().ok_or_else(malformed)?;
        let first_seq = r.u64().ok_or_else(malformed)?;
        let last_seq = r.u64().ok_or_else(malformed)?;
        let checks = [
            (VersionField::Format, format, FORMAT_VERSION),
            (VersionField::Schema, schema, SCHEMA_VERSION),
            (
                VersionField::Codec,
                u16::from(codec),
                u16::from(CODEC_VERSION),
            ),
        ];
        for (field, found, supported) in checks {
            if found > supported || found == 0 {
                let path = path.to_path_buf();
                return Err(StorageError::UnsupportedVersion {
                    path,
                    field,
                    found,
                    supported,
                });
            }
        }
        Ok(Self {
            stream: StreamId::new(stream),
            first_seq,
            last_seq,
            generation,
            codec_version: codec,
        })
    }
}
