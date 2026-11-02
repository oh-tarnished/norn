//! Gorilla XOR compression (Pelkonen et al., VLDB 2015, §4.1.2), generic
//! over `f64` and `f32` bit patterns.
//!
//! Each value is XORed with its predecessor. An unchanged value costs one
//! bit. A changed one costs a control bit pair and the XOR's meaningful bits,
//! reusing the previous leading/trailing-zero window when it still fits.
//! Operating on raw bits makes it lossless for every pattern: NaN payloads,
//! signed zeros, infinities and subnormals all round-trip exactly.
//!
//! Layout after the block header: the first value's raw bits, then the
//! bit stream, zero-padded to a byte.

use super::Float;
use crate::bits::{BitReader, BitWriter};
use crate::error::{CodecError, Result};
use crate::wire::Cursor;

/// Leading-zero counts are stored in 5 bits and capped at 31, as in the paper.
const LZ_BITS: u32 = 5;

/// Appends the Gorilla stream for `values` (bit patterns of `F`).
pub(crate) fn encode<F: Float>(values: &[u64], out: &mut Vec<u8>) {
    let Some((first, rest)) = values.split_first() else {
        return;
    };
    let mut w = BitWriter::new(out);
    w.write(*first, F::BITS);
    let (mut prev, mut lead, mut trail) = (*first, u32::MAX, 0u32);
    for v in rest {
        let x = *v ^ prev;
        prev = *v;
        if x == 0 {
            w.bit(false);
            continue;
        }
        w.bit(true);
        let lz = (x.leading_zeros() - (64 - F::BITS)).min(31);
        let tz = x.trailing_zeros();
        if lead != u32::MAX && lz >= lead && tz >= trail {
            w.bit(false);
            w.write(x >> trail, F::BITS - lead - trail);
        } else {
            let sig = F::BITS - lz - tz;
            w.bit(true);
            w.write(u64::from(lz), LZ_BITS);
            w.write(u64::from(sig - 1), F::SIG_BITS);
            w.write(x >> tz, sig);
            (lead, trail) = (lz, tz);
        }
    }
    w.finish();
}

/// Reads `n` values' bit patterns, appending them to `out`.
pub(crate) fn decode<F: Float>(cur: &mut Cursor<'_>, n: usize, out: &mut Vec<u64>) -> Result<()> {
    if n == 0 {
        return Ok(());
    }
    let base = cur.pos();
    let mut r = BitReader::new(cur.rest(), base);
    let mut prev = r.read(F::BITS)?;
    out.push(prev);
    let (mut lead, mut trail) = (u32::MAX, 0u32);
    for _ in 1..n {
        if r.bit()? {
            if r.bit()? {
                let at = base + r.consumed();
                lead = r.read(LZ_BITS)? as u32;
                let sig = r.read(F::SIG_BITS)? as u32 + 1;
                if lead + sig > F::BITS {
                    return Err(CodecError::Corrupt {
                        offset: at,
                        reason: "XOR window wider than value",
                    });
                }
                trail = F::BITS - lead - sig;
            } else if lead == u32::MAX {
                let offset = base + r.consumed();
                return Err(CodecError::Corrupt {
                    offset,
                    reason: "XOR window reused before set",
                });
            }
            prev ^= r.read(F::BITS - lead - trail)? << trail;
        }
        out.push(prev);
    }
    cur.take(r.consumed())?;
    Ok(())
}
