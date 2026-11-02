//! Floating-point columns: Gorilla XOR and ALP, chosen per block by
//! measurement.
//!
//! [`encode_auto`] encodes the block both ways and keeps the smaller. That
//! doubles encode cost, which the benchmarks in docs/reference/storage.md show is still well
//! above ingest rates; in exchange a block of decimal-like readings gets
//! ALP's integer encoding and a block of genuinely binary noise keeps
//! Gorilla, with no configuration.

pub(crate) mod alp;
pub(crate) mod api;
pub(crate) mod gorilla;

use crate::error::{CodecError, Result};
use crate::header;
use crate::id::{CodecId, ColumnType};
use crate::wire::Cursor;

/// The float widths the codecs support. Sealed to this crate.
pub(crate) trait Float: Copy {
    /// Bits in the representation.
    const BITS: u32;
    /// Bytes in the representation.
    const BYTES: usize;
    /// Bits Gorilla uses to store a meaningful-bit count minus one.
    const SIG_BITS: u32;
    /// Largest decimal exponent ALP tries.
    const MAX_EXP: u8;
    /// The column and its two codecs.
    const COLUMN: ColumnType;
    const GORILLA: CodecId;
    const ALP: CodecId;
    fn bits(self) -> u64;
    fn from_bits(bits: u64) -> Self;
    fn to_f64(self) -> f64;
    fn from_f64(v: f64) -> Self;
}

impl Float for f64 {
    const BITS: u32 = 64;
    const BYTES: usize = 8;
    const SIG_BITS: u32 = 6;
    const MAX_EXP: u8 = 18;
    const COLUMN: ColumnType = ColumnType::Float64;
    const GORILLA: CodecId = CodecId::Float64Gorilla;
    const ALP: CodecId = CodecId::Float64Alp;
    fn bits(self) -> u64 {
        self.to_bits()
    }
    fn from_bits(bits: u64) -> Self {
        f64::from_bits(bits)
    }
    fn to_f64(self) -> f64 {
        self
    }
    fn from_f64(v: f64) -> Self {
        v
    }
}

impl Float for f32 {
    const BITS: u32 = 32;
    const BYTES: usize = 4;
    const SIG_BITS: u32 = 5;
    const MAX_EXP: u8 = 10;
    const COLUMN: ColumnType = ColumnType::Float32;
    const GORILLA: CodecId = CodecId::Float32Gorilla;
    const ALP: CodecId = CodecId::Float32Alp;
    fn bits(self) -> u64 {
        u64::from(self.to_bits())
    }
    fn from_bits(bits: u64) -> Self {
        f32::from_bits(bits as u32)
    }
    fn to_f64(self) -> f64 {
        f64::from(self)
    }
    fn from_f64(v: f64) -> Self {
        v as f32
    }
}

/// Encodes with whichever of Gorilla and ALP is smaller.
pub(crate) fn encode_auto<F: Float>(values: &[F], out: &mut Vec<u8>) -> Result<CodecId> {
    header::check_len(values.len())?;
    let mut gorilla = Vec::new();
    encode_with(F::GORILLA, values, &mut gorilla)?;
    let mut alp = Vec::new();
    encode_with(F::ALP, values, &mut alp)?;
    let (codec, best) = match alp.len() < gorilla.len() {
        true => (F::ALP, alp),
        false => (F::GORILLA, gorilla),
    };
    out.extend_from_slice(&best);
    Ok(codec)
}

/// Encodes with a forced float codec.
pub(crate) fn encode_with<F: Float>(codec: CodecId, values: &[F], out: &mut Vec<u8>) -> Result<()> {
    header::check_len(values.len())?;
    header::check_column(codec, F::COLUMN)?;
    header::write(out, codec, values.len());
    if codec == F::ALP {
        alp::encode(values, out)
    } else {
        let bits: Vec<u64> = values.iter().map(|v| v.bits()).collect();
        gorilla::encode::<F>(&bits, out);
        Ok(())
    }
}

/// Decodes one block, appending to `out`.
pub(crate) fn decode<F: Float>(block: &[u8], out: &mut Vec<F>) -> Result<()> {
    let mut cur = Cursor::new(block);
    let h = header::read_for(&mut cur, F::COLUMN)?;
    out.reserve(h.count);
    if h.codec == F::ALP {
        alp::decode(&mut cur, h.count, out)?;
    } else if h.codec == F::GORILLA {
        let mut bits = Vec::with_capacity(h.count);
        gorilla::decode::<F>(&mut cur, h.count, &mut bits)?;
        out.extend(bits.into_iter().map(F::from_bits));
    } else {
        return Err(CodecError::NotApplicable { codec: h.codec });
    }
    cur.finish()
}
