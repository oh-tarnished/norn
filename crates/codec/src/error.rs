//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The codec's typed error.
//!
//! Every decode failure carries the byte offset, within the block handed to
//! the decoder, at which the problem was found, so a corrupted chunk can be
//! reported precisely (SPEC §20) instead of with a message to grep.

use core::fmt;

use crate::id::{CodecId, ColumnType};

/// Why encoding or decoding a block failed.
///
/// Callers match on the variant; the `Display` text is for humans only.
/// The type is `Send + Sync + 'static` and cheap to clone.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum CodecError {
    /// The block ended before a field that must be present. `needed` is the
    /// number of further bytes the decoder required at `offset`.
    Truncated {
        /// Byte offset where the read began.
        offset: usize,
        /// Bytes required from `offset`.
        needed: usize,
    },
    /// The header names a codec id this build does not know.
    UnknownCodec {
        /// Byte offset of the id.
        offset: usize,
        /// The unrecognised id.
        id: u8,
    },
    /// The codec is known but this build cannot read the stored
    /// `codec_version` (SPEC §40: fail clearly, never misread).
    UnsupportedVersion {
        /// Byte offset of the version.
        offset: usize,
        /// The codec named by the header.
        codec: CodecId,
        /// The stored version.
        version: u8,
    },
    /// The block is valid but belongs to a different column type than the
    /// decoder (or the forced encoder) asked for.
    WrongColumn {
        /// Byte offset of the codec id (0 when encoding).
        offset: usize,
        /// The codec found or requested.
        codec: CodecId,
        /// The column type the caller expected.
        expected: ColumnType,
    },
    /// A count or length exceeds a declared bound in [`crate::limits`].
    TooLarge {
        /// Byte offset of the field (0 when encoding).
        offset: usize,
        /// The value found.
        len: u64,
        /// The maximum allowed.
        max: u64,
    },
    /// A field holds a value no encoder produces.
    Corrupt {
        /// Byte offset of the field.
        offset: usize,
        /// What was wrong, for humans.
        reason: &'static str,
    },
    /// The block decoded completely but bytes remain after it.
    TrailingBytes {
        /// Offset of the first unconsumed byte.
        offset: usize,
    },
    /// A forced codec cannot represent the given values losslessly (for
    /// example [`CodecId::TimestampRegular`] on irregular timestamps).
    NotApplicable {
        /// The codec requested.
        codec: CodecId,
    },
    /// The zstd frame inside a compressed envelope did not decode to the
    /// recorded length.
    Decompress {
        /// Byte offset of the frame within the envelope.
        offset: usize,
        /// What the decompressor reported, for humans.
        reason: &'static str,
    },
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Truncated { offset, needed } => {
                write!(
                    f,
                    "block truncated at byte {offset}: {needed} more bytes needed"
                )
            }
            Self::UnknownCodec { offset, id } => {
                write!(f, "unknown codec id {id:#04x} at byte {offset}")
            }
            Self::UnsupportedVersion {
                offset,
                codec,
                version,
            } => {
                write!(
                    f,
                    "{codec:?} version {version} at byte {offset} is not supported"
                )
            }
            Self::WrongColumn {
                offset,
                codec,
                expected,
            } => {
                write!(f, "{codec:?} at byte {offset} is not a {expected:?} codec")
            }
            Self::TooLarge { offset, len, max } => {
                write!(f, "length {len} at byte {offset} exceeds the limit {max}")
            }
            Self::Corrupt { offset, reason } => {
                write!(f, "corrupt block at byte {offset}: {reason}")
            }
            Self::TrailingBytes { offset } => write!(f, "unexpected bytes after block at {offset}"),
            Self::NotApplicable { codec } => write!(f, "{codec:?} cannot encode these values"),
            Self::Decompress { offset, reason } => {
                write!(f, "zstd frame at byte {offset} failed: {reason}")
            }
        }
    }
}

impl std::error::Error for CodecError {}

/// Shorthand used throughout the crate.
pub type Result<T> = core::result::Result<T, CodecError>;
