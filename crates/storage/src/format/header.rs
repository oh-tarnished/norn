//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The segment header: identity and versions, checked before anything else.

use norn_codec::CODEC_VERSION;
use norn_core::StreamId;

use super::{FORMAT_MAJOR, HEADER_LEN, SCHEMA_VERSION, SEGMENT_MAGIC, sealing};
use crate::crc32c::crc32c;
use crate::crypt::EncryptionInfo;

/// Header flag 0x01, format 1.1: the encryption record follows the header.
pub(super) const FLAG_ENCRYPTED: u8 = 0x01;

/// The first [`HEADER_LEN`] bytes of a segment (SPEC §40).
///
/// | Offset | Size | Field |
/// | ---: | ---: | --- |
/// | 0 | 8 | magic `NORNSEG\0` |
/// | 8 | 1 | format major version, 1 |
/// | 9 | 1 | format minor version: 0, or 1 when encrypted |
/// | 10 | 2 | `schema_version` |
/// | 12 | 1 | `codec_version` |
/// | 13 | 1 | flags: zero in 1.0; in 1.1, 0x01 = encrypted |
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
/// Bytes 8..10 read as one little-endian `u16` are 1 for format 1.0, the only
/// value 1.0.0-rc.1 accepts, and 257 for 1.1, which that build therefore refuses
/// as `UnsupportedVersion` and never modifies. A plain segment is still written
/// as 1.0, byte for byte as before; only an encrypted one is 1.1, with the
/// [`sealing`] record after the header and its first chunk at byte 96.
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
    /// How its chunk bodies are encrypted; `None` for a plain segment. Set by
    /// the writer from its options, never by a caller.
    pub encryption: Option<EncryptionInfo>,
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
            encryption: None,
        }
    }

    /// Where its first chunk frame starts: after the header, and after the
    /// encryption record when there is one.
    #[must_use]
    pub fn data_start(&self) -> u64 {
        let record = if self.encryption.is_some() {
            sealing::SEALING_LEN
        } else {
            0
        };
        (HEADER_LEN + record) as u64
    }

    /// Whether `head`, a header not yet verified, says an encryption record
    /// follows: only which bytes to read next, and [`decode`](Self::decode) decides.
    pub(crate) fn has_record(head: &[u8]) -> bool {
        let at = |i| head.get(i).copied().unwrap_or(0);
        at(9) >= 1 && at(13) & FLAG_ENCRYPTED != 0
    }

    /// The header's bytes and, when encrypted, the encryption record's: the
    /// first [`data_start`](SegmentHeader::data_start) bytes of the file.
    #[must_use]
    pub fn encode_all(&self) -> Vec<u8> {
        let mut out = self.encode().to_vec();
        if let Some(info) = &self.encryption {
            out.extend_from_slice(&sealing::encode(info));
        }
        out
    }

    /// The header's own [`HEADER_LEN`] bytes, CRC included, without the
    /// encryption record ([`encode_all`](SegmentHeader::encode_all) has both).
    #[must_use]
    pub fn encode(&self) -> [u8; HEADER_LEN] {
        let (minor, flags) = match self.encryption {
            Some(_) => (1, FLAG_ENCRYPTED),
            None => (0, 0),
        };
        let mut b = Vec::with_capacity(HEADER_LEN);
        b.extend_from_slice(&SEGMENT_MAGIC);
        b.extend([FORMAT_MAJOR, minor]);
        b.extend_from_slice(&SCHEMA_VERSION.to_le_bytes());
        b.extend([self.codec_version, flags]);
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
}
