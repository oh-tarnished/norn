//! Public entry points for the timestamp and integer columns.
//!
//! Both columns share the `i64` family in [`crate::int`]; they differ only in
//! codec ids, so a block from one cannot be decoded as the other.

/// Timestamp columns: signed 64-bit instants in any unit.
///
/// Typical results on 1 Hz nanosecond timestamps: a perfectly regular block
/// is 4 + header bytes in total (start + step); occasional jitter or gaps
/// select delta-of-delta, where an on-time sample costs its miniblock's
/// width (often 0 bits) and a gap widens only its own 128-value miniblock.
pub mod timestamp {
    use crate::error::Result;
    use crate::id::{CodecId, ColumnType};
    use crate::int;

    /// Encodes `values` with the smallest timestamp codec, appending one
    /// self-describing block to `out` and returning the codec chosen.
    ///
    /// Performance: O(n) time; allocates O(n) scratch to measure each
    /// candidate. Any order of values is accepted; sorted input compresses
    /// best.
    ///
    /// # Errors
    /// [`crate::CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
    pub fn encode(values: &[i64], out: &mut Vec<u8>) -> Result<CodecId> {
        int::encode_auto(ColumnType::Timestamp, values, out)
    }

    /// Encodes with a forced codec, for benchmarks and tests.
    ///
    /// # Errors
    /// As [`encode`], plus [`crate::CodecError::WrongColumn`] for a
    /// non-timestamp codec and [`crate::CodecError::NotApplicable`] when
    /// [`CodecId::TimestampRegular`] is forced onto an irregular series.
    pub fn encode_with(codec: CodecId, values: &[i64], out: &mut Vec<u8>) -> Result<()> {
        int::encode_with(ColumnType::Timestamp, codec, values, out)
    }

    /// Decodes one block, appending its values to `out`. The block is read
    /// in place (nothing is borrowed past the call); output is copied.
    ///
    /// # Errors
    /// Any [`crate::CodecError`] decode variant; never panics on corrupt input.
    pub fn decode(block: &[u8], out: &mut Vec<i64>) -> Result<()> {
        int::decode(ColumnType::Timestamp, block, out)
    }
}

/// Integer columns: signed 64-bit values. Narrower and unsigned integers
/// widen losslessly (`u64` via `as i64` bit-reinterpretation).
///
/// Monotonic counters select delta or delta-of-delta; noisy values select
/// frame-of-reference packing; values that rarely change select run-length.
pub mod integer {
    use crate::error::Result;
    use crate::id::{CodecId, ColumnType};
    use crate::int;

    /// Encodes `values` with the smallest integer codec, appending one
    /// self-describing block to `out` and returning the codec chosen.
    ///
    /// Performance: O(n) time and O(n) scratch.
    ///
    /// # Errors
    /// [`crate::CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
    pub fn encode(values: &[i64], out: &mut Vec<u8>) -> Result<CodecId> {
        int::encode_auto(ColumnType::Integer, values, out)
    }

    /// Encodes with a forced codec, for benchmarks and tests.
    ///
    /// # Errors
    /// As [`encode`], plus [`crate::CodecError::WrongColumn`] and
    /// [`crate::CodecError::NotApplicable`].
    pub fn encode_with(codec: CodecId, values: &[i64], out: &mut Vec<u8>) -> Result<()> {
        int::encode_with(ColumnType::Integer, codec, values, out)
    }

    /// Decodes one block, appending its values to `out`; copies, borrows
    /// nothing past the call.
    ///
    /// # Errors
    /// Any [`crate::CodecError`] decode variant; never panics on corrupt input.
    pub fn decode(block: &[u8], out: &mut Vec<i64>) -> Result<()> {
        int::decode(ColumnType::Integer, block, out)
    }
}
