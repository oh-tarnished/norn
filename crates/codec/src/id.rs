//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Codec identifiers and the column types they belong to.
//!
//! The id is the first byte of every encoded block and is part of the stored
//! format (SPEC §40): an id, once assigned, is never reused for a different
//! layout. A changed layout under the same id bumps [`CODEC_VERSION`].

/// The layout version written after the codec id. Version 1 is the only
/// one this build reads; decoding any other is
/// [`crate::CodecError::UnsupportedVersion`].
pub const CODEC_VERSION: u8 = 1;

/// The logical type of a column. Each [`CodecId`] serves exactly one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ColumnType {
    /// Signed 64-bit timestamps (any unit; the codec does not interpret it).
    Timestamp,
    /// Signed 64-bit integers. Narrower or unsigned ints widen losslessly.
    Integer,
    /// Booleans.
    Boolean,
    /// Enumeration codes (`u32`), typically multi-state values.
    Enum,
    /// IEEE-754 binary64.
    Float64,
    /// IEEE-754 binary32.
    Float32,
    /// Byte strings (UTF-8 strings are stored as their bytes).
    Bytes,
}

/// Every encoding this build knows, with its stored id.
///
/// The `i64` family (regular, delta, delta-of-delta, frame-of-reference,
/// run-length) exists once per column type that uses it, so a timestamp
/// block can never be decoded as an integer column by mistake.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CodecId {
    /// Timestamps as start + step; every value is `start + i * step`.
    TimestampRegular = 0x01,
    /// Timestamps as first value + bit-packed deltas.
    TimestampDelta = 0x02,
    /// Timestamps as first value, first delta + bit-packed second deltas.
    TimestampDeltaOfDelta = 0x03,
    /// Timestamps bit-packed against a per-miniblock minimum.
    TimestampFor = 0x04,
    /// Timestamps as runs of equal values.
    TimestampRle = 0x05,
    /// Integers as start + step.
    IntegerRegular = 0x11,
    /// Integers as first value + bit-packed deltas.
    IntegerDelta = 0x12,
    /// Integers as delta-of-delta.
    IntegerDeltaOfDelta = 0x13,
    /// Integers bit-packed against a per-miniblock minimum.
    IntegerFor = 0x14,
    /// Integers as runs of equal values.
    IntegerRle = 0x15,
    /// Booleans packed one bit each.
    BooleanBits = 0x21,
    /// Booleans as a first value and alternating run lengths.
    BooleanRle = 0x22,
    /// Enum codes via a dictionary and bit-packed indices.
    EnumDict = 0x31,
    /// Enum codes via a dictionary and run-length-encoded indices.
    EnumDictRle = 0x32,
    /// `f64` with Gorilla XOR compression.
    Float64Gorilla = 0x41,
    /// `f64` with ALP (decimal digits + exceptions).
    Float64Alp = 0x42,
    /// `f32` with Gorilla XOR compression.
    Float32Gorilla = 0x49,
    /// `f32` with ALP.
    Float32Alp = 0x4a,
    /// Byte strings, length-prefixed.
    BytesPlain = 0x51,
    /// Byte strings via a dictionary of distinct values.
    BytesDict = 0x52,
}

/// Every codec, for exhaustive tests and inspection tools.
pub const ALL_CODECS: [CodecId; 20] = {
    use CodecId::*;
    [
        TimestampRegular,
        TimestampDelta,
        TimestampDeltaOfDelta,
        TimestampFor,
        TimestampRle,
        IntegerRegular,
        IntegerDelta,
        IntegerDeltaOfDelta,
        IntegerFor,
        IntegerRle,
        BooleanBits,
        BooleanRle,
        EnumDict,
        EnumDictRle,
        Float64Gorilla,
        Float64Alp,
        Float32Gorilla,
        Float32Alp,
        BytesPlain,
        BytesDict,
    ]
};

impl CodecId {
    /// The id for a stored byte, or `None` if this build does not know it.
    pub fn from_u8(id: u8) -> Option<Self> {
        ALL_CODECS.iter().copied().find(|c| *c as u8 == id)
    }

    /// The column type this codec encodes.
    pub fn column(self) -> ColumnType {
        match self as u8 >> 4 {
            0x0 => ColumnType::Timestamp,
            0x1 => ColumnType::Integer,
            0x2 => ColumnType::Boolean,
            0x3 => ColumnType::Enum,
            0x4 if (self as u8) < 0x48 => ColumnType::Float64,
            0x4 => ColumnType::Float32,
            _ => ColumnType::Bytes,
        }
    }
}
