//! Public entry points for `f64` and `f32` columns.

/// `f64` columns: Gorilla XOR or ALP, whichever is smaller per block.
///
/// Lossless on bit patterns: NaN payloads, `-0.0`, infinities and
/// subnormals decode to the identical bits. Decimal-like sensor readings
/// (0.1 or 0.25 resolution) select ALP; full-precision noise selects Gorilla.
pub mod float64 {
    use crate::error::Result;
    use crate::float;
    use crate::id::CodecId;

    /// Encodes `values` with the smaller float codec, appending one block.
    ///
    /// Performance: encodes twice (once per codec) and keeps the smaller;
    /// ALP's exponent search samples at most 256 values. O(n) time and
    /// O(n) scratch.
    ///
    /// # Errors
    /// [`crate::CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
    pub fn encode(values: &[f64], out: &mut Vec<u8>) -> Result<CodecId> {
        float::encode_auto(values, out)
    }

    /// Encodes with a forced codec ([`CodecId::Float64Gorilla`] or
    /// [`CodecId::Float64Alp`]); both accept every input.
    ///
    /// # Errors
    /// As [`encode`], plus [`crate::CodecError::WrongColumn`].
    pub fn encode_with(codec: CodecId, values: &[f64], out: &mut Vec<u8>) -> Result<()> {
        float::encode_with(codec, values, out)
    }

    /// Decodes one block, appending to `out`; copies, borrows nothing.
    ///
    /// # Errors
    /// Any decode variant of [`crate::CodecError`]; never panics.
    pub fn decode(block: &[u8], out: &mut Vec<f64>) -> Result<()> {
        float::decode(block, out)
    }
}

/// `f32` columns: as [`float64`], with 32-bit patterns and ALP exponents
/// up to 10.
pub mod float32 {
    use crate::error::Result;
    use crate::float;
    use crate::id::CodecId;

    /// Encodes `values` with the smaller float codec, appending one block.
    ///
    /// Performance: as [`super::float64::encode`].
    ///
    /// # Errors
    /// [`crate::CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
    pub fn encode(values: &[f32], out: &mut Vec<u8>) -> Result<CodecId> {
        float::encode_auto(values, out)
    }

    /// Encodes with a forced codec ([`CodecId::Float32Gorilla`] or
    /// [`CodecId::Float32Alp`]).
    ///
    /// # Errors
    /// As [`encode`], plus [`crate::CodecError::WrongColumn`].
    pub fn encode_with(codec: CodecId, values: &[f32], out: &mut Vec<u8>) -> Result<()> {
        float::encode_with(codec, values, out)
    }

    /// Decodes one block, appending to `out`; copies, borrows nothing.
    ///
    /// # Errors
    /// Any decode variant of [`crate::CodecError`]; never panics.
    pub fn decode(block: &[u8], out: &mut Vec<f32>) -> Result<()> {
        float::decode(block, out)
    }
}
