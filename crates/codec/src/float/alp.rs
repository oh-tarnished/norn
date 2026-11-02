//! ALP: Adaptive Lossless floating-Point compression (Afroozeh, Kuffó and
//! Boncz, SIGMOD 2024), the scalar variant without vectorised kernels.
//!
//! Sensor values are usually decimals written with a fixed resolution:
//! 21.5, 21.6, 0.25. ALP finds exponents `e >= f` such that
//! `d = round(v * 10^e * 10^-f)` is an integer from which
//! `d * 10^f * 10^-e` reproduces `v` *bit for bit*; the integers `d` then go
//! through the integer codec (delta, frame-of-reference, RLE) which stores a
//! slow 0.1-resolution drift in a few bits per sample. Values that fail the
//! exact check (NaN, -0.0, infinities, true binary fractions) are
//! exceptions, stored raw by position.
//!
//! Layout after the block header: `e: u8`, `f: u8`, exception count
//! (varint), exception positions (delta-packed), exception raw bits (little
//! endian, 8 or 4 bytes each), then a nested integer block of the digits.

use super::Float;
use crate::error::{CodecError, Result};
use crate::id::ColumnType;
use crate::int::{self, algo};
use crate::wire::{Cursor, put_varint};

/// `10^i` for `i` in 0..=18, each exactly the nearest `f64`.
const F10: [f64; 19] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18,
];
/// `10^-i` for `i` in 0..=18; multiplying by these (not dividing) is the
/// paper's formulation, and the decoder uses the same constants.
const IF10: [f64; 19] = [
    1e0, 1e-1, 1e-2, 1e-3, 1e-4, 1e-5, 1e-6, 1e-7, 1e-8, 1e-9, 1e-10, 1e-11, 1e-12, 1e-13, 1e-14,
    1e-15, 1e-16, 1e-17, 1e-18,
];
/// Values sampled when searching for `(e, f)`.
const SAMPLE: usize = 256;

fn pow(t: &[f64; 19], i: u8) -> f64 {
    t.get(usize::from(i)).copied().unwrap_or(1.0)
}

/// The decoded value for digit `d` under `(e, f)`.
fn undigit<F: Float>(d: i64, e: u8, f: u8) -> F {
    F::from_f64(d as f64 * pow(&F10, f) * pow(&IF10, e))
}

/// The digit for `v` if it round-trips exactly under `(e, f)`.
fn digit<F: Float>(v: F, e: u8, f: u8) -> Option<i64> {
    let d = (v.to_f64() * pow(&F10, e) * pow(&IF10, f)).round();
    if d.is_nan() || d.abs() >= 9.0e18 {
        return None;
    }
    let d = d as i64;
    (undigit::<F>(d, e, f).bits() == v.bits()).then_some(d)
}

/// The `(e, f)` pair that minimises estimated size over a sample.
pub(crate) fn search<F: Float>(values: &[F]) -> (u8, u8) {
    let step = (values.len() / SAMPLE).max(1);
    let sample: Vec<F> = values.iter().step_by(step).copied().collect();
    let mut best = (u64::MAX, 0u8, 0u8);
    for e in 0..=F::MAX_EXP {
        for f in 0..=e {
            let (mut exc, mut lo, mut hi) = (0u64, i64::MAX, i64::MIN);
            for v in &sample {
                match digit(*v, e, f) {
                    Some(d) => (lo, hi) = (lo.min(d), hi.max(d)),
                    None => exc += 1,
                }
            }
            let width = u64::from(crate::bits::width_of(hi.wrapping_sub(lo) as u64));
            let cost = exc * (u64::from(F::BITS) + 16) + (sample.len() as u64 - exc) * width;
            if cost < best.0 {
                best = (cost, e, f);
            }
        }
    }
    (best.1, best.2)
}

/// Appends the ALP payload for `values`.
pub(crate) fn encode<F: Float>(values: &[F], out: &mut Vec<u8>) -> Result<()> {
    let (e, f) = search(values);
    let (mut digits, mut pos, mut raw) = (Vec::with_capacity(values.len()), Vec::new(), Vec::new());
    let mut last = 0i64;
    for (i, v) in values.iter().enumerate() {
        match digit(*v, e, f) {
            Some(d) => last = d,
            None => {
                pos.push(i as i64);
                raw.push(v.bits());
            }
        }
        digits.push(last);
    }
    out.extend([e, f]);
    put_varint(out, pos.len() as u64);
    algo::encode(algo::Algo::Delta, &pos, out);
    for bits in raw {
        out.extend_from_slice(bits.to_le_bytes().get(..F::BYTES).unwrap_or_default());
    }
    int::encode_auto(ColumnType::Integer, &digits, out).map(|_| ())
}

/// Reads `n` values, appending them to `out`.
pub(crate) fn decode<F: Float>(cur: &mut Cursor<'_>, n: usize, out: &mut Vec<F>) -> Result<()> {
    let at = cur.pos();
    let (e, f) = (cur.u8()?, cur.u8()?);
    if e > F::MAX_EXP || f > e {
        return Err(CodecError::Corrupt {
            offset: at,
            reason: "ALP exponents out of range",
        });
    }
    let nexc = cur.count(n)?;
    let mut pos = Vec::with_capacity(nexc);
    let pos_at = cur.pos();
    algo::decode(algo::Algo::Delta, cur, nexc, &mut pos)?;
    let mut raw = Vec::with_capacity(nexc);
    for _ in 0..nexc {
        raw.push(cur.uint_le(F::BYTES)?);
    }
    let digits_at = cur.pos();
    let mut digits = Vec::with_capacity(n);
    int::decode_from(ColumnType::Integer, cur, &mut digits)?;
    if digits.len() != n {
        return Err(CodecError::Corrupt {
            offset: digits_at,
            reason: "digit count mismatch",
        });
    }
    let start = out.len();
    out.extend(digits.into_iter().map(|d| undigit::<F>(d, e, f)));
    for (p, bits) in pos.into_iter().zip(raw) {
        let slot = usize::try_from(p)
            .ok()
            .and_then(|p| out.get_mut(start.checked_add(p)?));
        let slot = slot.ok_or(CodecError::Corrupt {
            offset: pos_at,
            reason: "exception outside block",
        })?;
        *slot = F::from_bits(bits);
    }
    Ok(())
}
