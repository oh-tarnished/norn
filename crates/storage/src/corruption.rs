//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The description of one damaged region, shared by read errors and recovery
//! events, so both describe the same damage identically (SPEC §20).

use std::path::PathBuf;

/// What was wrong with a checksummed or structured region of a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CorruptionKind {
    /// The segment header's magic is not `NORNSEG\0`: not a segment file.
    BadMagic,
    /// The segment header's CRC does not match its bytes.
    HeaderChecksum,
    /// A chunk frame header's CRC does not match, or its magic is missing.
    FrameHeader,
    /// A chunk body's CRC does not match the one in its frame header.
    BodyChecksum {
        /// The CRC recorded when the chunk was written.
        stored: u32,
        /// The CRC of the bytes read now.
        computed: u32,
    },
    /// The file ends inside a frame: an incomplete (torn) write.
    TornTail,
    /// The chunk directory's CRC or framing does not match.
    Directory,
    /// The directory disagrees with the frames it points at.
    DirectoryMismatch,
    /// A checksum-valid body whose structure is impossible (bad column
    /// length, value count mismatch): a writer bug or a CRC collision.
    Malformed(&'static str),
}

/// One corrupt region: where it is and what is wrong with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Corruption {
    /// The segment file.
    pub path: PathBuf,
    /// Byte offset of the region within the file.
    pub offset: u64,
    /// Length of the region in bytes (to end of file for a torn tail).
    pub len: u64,
    /// What is wrong.
    pub kind: CorruptionKind,
}
