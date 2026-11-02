//! Patched frame-of-reference bit packing of `i64` in miniblocks.
//!
//! Each miniblock of up to [`MINIBLOCK`] values stores a reference `base`
//! (zigzag varint) and a flag byte: the bit width in its low 7 bits and, in
//! bit 7, whether exceptions follow. Every value is packed as `v - base` at
//! that width; exception slots pack as 0 and are then patched from a list of
//! `count: u8`, `count` one-byte positions, and `count` zigzag varints of
//! `v - base`. Subtraction wraps, so the full `i64` range is lossless.
//!
//! Performance: planning sorts each 128-value miniblock and tries up to 153
//! trims; constant miniblocks cost two to three bytes per 128 values.

use super::plan;
use crate::bits::{BitReader, BitWriter, packed_len};
use crate::error::{CodecError, Result};
use crate::limits::MINIBLOCK;
use crate::wire::{Cursor, put_ivarint};

pub(crate) use super::plan::varint_len;

/// Flag bit marking a patched miniblock.
const PATCHED: u8 = 0x80;

/// Appends `values` packed in miniblocks. The count is not written; the
/// caller records it.
pub(crate) fn encode(values: &[i64], out: &mut Vec<u8>) {
    for chunk in values.chunks(MINIBLOCK) {
        let p = plan::plan(chunk);
        let exc: Vec<(usize, i64)> = chunk
            .iter()
            .copied()
            .enumerate()
            .filter(|(_, v)| p.is_exception(*v))
            .collect();
        put_ivarint(out, p.base);
        out.push(p.width as u8 | if exc.is_empty() { 0 } else { PATCHED });
        let mut w = BitWriter::new(out);
        for v in chunk {
            let packed = if p.is_exception(*v) {
                0
            } else {
                v.wrapping_sub(p.base) as u64
            };
            w.write(packed, p.width);
        }
        w.finish();
        if !exc.is_empty() {
            out.push(exc.len() as u8);
            out.extend(exc.iter().map(|(i, _)| *i as u8));
            exc.iter()
                .for_each(|(_, v)| put_ivarint(out, v.wrapping_sub(p.base)));
        }
    }
}

/// Reads `count` packed values, appending them to `out`.
///
/// # Errors
/// Truncation, a width above 64, or an exception list that is empty, longer
/// than its miniblock, or points outside it.
pub(crate) fn decode(cur: &mut Cursor<'_>, count: usize, out: &mut Vec<i64>) -> Result<()> {
    let mut left = count;
    while left > 0 {
        let n = left.min(MINIBLOCK);
        let base = cur.ivarint()?;
        let at = cur.pos();
        let flag = cur.u8()?;
        let width = u32::from(flag & !PATCHED);
        if width > 64 {
            return Err(CodecError::Corrupt {
                offset: at,
                reason: "bit width above 64",
            });
        }
        let len = packed_len(n, width).unwrap_or(usize::MAX);
        let (start, offset) = (out.len(), cur.pos());
        let mut r = BitReader::new(cur.take(len)?, offset);
        for _ in 0..n {
            out.push(base.wrapping_add(r.read(width)? as i64));
        }
        if flag & PATCHED != 0 {
            patch(cur, base, n, out.get_mut(start..).unwrap_or_default())?;
        }
        left -= n;
    }
    Ok(())
}

/// Applies a miniblock's exception list to its decoded values.
fn patch(cur: &mut Cursor<'_>, base: i64, n: usize, block: &mut [i64]) -> Result<()> {
    let at = cur.pos();
    let count = usize::from(cur.u8()?);
    if count == 0 || count > n {
        return Err(CodecError::Corrupt {
            offset: at,
            reason: "exception count out of range",
        });
    }
    let positions = cur.take(count)?;
    for (i, pos) in positions.iter().enumerate() {
        let v = base.wrapping_add(cur.ivarint()?);
        let slot = block.get_mut(usize::from(*pos));
        let offset = at + 1 + i;
        *slot.ok_or(CodecError::Corrupt {
            offset,
            reason: "exception outside miniblock",
        })? = v;
    }
    Ok(())
}

/// Bytes [`encode`] would produce for `values`, exactly, without writing.
/// Used to choose between codecs.
pub(crate) fn size(values: &[i64]) -> usize {
    values
        .chunks(MINIBLOCK)
        .map(|chunk| plan::plan(chunk).size(chunk))
        .fold(0usize, usize::saturating_add)
}
