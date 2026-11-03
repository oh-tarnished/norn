//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Boolean columns: one bit per value, or alternating run lengths.
//!
//! A binary point that toggles rarely (a pump on/off, an alarm) is a handful
//! of runs per day, so RLE stores it in a few bytes regardless of sample
//! count; a value that flips often is cheaper as plain bits (1/8 byte each).
//! [`encode`] measures both and keeps the smaller.

use crate::bits::{BitReader, BitWriter, packed_len};
use crate::error::{CodecError, Result};
use crate::header;
use crate::id::{CodecId, ColumnType};
use crate::int::pack;
use crate::wire::{Cursor, put_varint};

const COLUMN: ColumnType = ColumnType::Boolean;

/// Encodes `values` with the smaller boolean codec, appending one block.
///
/// Performance: O(n); allocates one `i64` per run to measure RLE.
///
/// # Errors
/// [`CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
pub fn encode(values: &[bool], out: &mut Vec<u8>) -> Result<CodecId> {
    let runs = runs(values);
    let rle = pack::varint_len(runs.len() as u64) + 1 + pack::size(&runs);
    let bits = packed_len(values.len(), 1).unwrap_or(usize::MAX);
    let codec = if rle < bits {
        CodecId::BooleanRle
    } else {
        CodecId::BooleanBits
    };
    encode_with(codec, values, out)?;
    Ok(codec)
}

/// Encodes with a forced boolean codec.
///
/// # Errors
/// [`CodecError::TooLarge`], or [`CodecError::WrongColumn`] for a
/// non-boolean codec.
pub fn encode_with(codec: CodecId, values: &[bool], out: &mut Vec<u8>) -> Result<()> {
    header::check_len(values.len())?;
    header::check_column(codec, COLUMN)?;
    header::write(out, codec, values.len());
    if codec == CodecId::BooleanRle {
        let runs = runs(values);
        put_varint(out, runs.len() as u64);
        out.push(u8::from(values.first().copied().unwrap_or(false)));
        pack::encode(&runs, out);
    } else {
        let mut w = BitWriter::new(out);
        values.iter().for_each(|b| w.bit(*b));
        w.finish();
    }
    Ok(())
}

/// Decodes one block, appending to `out`; copies, borrows nothing.
///
/// # Errors
/// Any decode variant of [`CodecError`]; never panics on corrupt input.
pub fn decode(block: &[u8], out: &mut Vec<bool>) -> Result<()> {
    let mut cur = Cursor::new(block);
    let h = header::read_for(&mut cur, COLUMN)?;
    out.reserve(h.count);
    if h.codec == CodecId::BooleanRle {
        decode_rle(&mut cur, h.count, out)?;
    } else {
        let len = packed_len(h.count, 1).unwrap_or(usize::MAX);
        let base = cur.pos();
        let mut r = BitReader::new(cur.take(len)?, base);
        for _ in 0..h.count {
            out.push(r.bit()?);
        }
    }
    cur.finish()
}

fn decode_rle(cur: &mut Cursor<'_>, n: usize, out: &mut Vec<bool>) -> Result<()> {
    let nruns = cur.count(n)?;
    let at = cur.pos();
    let mut value = match cur.u8()? {
        0 => false,
        1 => true,
        _ => {
            return Err(CodecError::Corrupt {
                offset: at,
                reason: "boolean is not 0 or 1",
            });
        }
    };
    let mut lens = Vec::with_capacity(nruns);
    let at = cur.pos();
    pack::decode(cur, nruns, &mut lens)?;
    let mut left = n;
    for len in lens {
        let len = usize::try_from(len).ok().filter(|l| *l >= 1 && *l <= left);
        let len = len.ok_or(CodecError::Corrupt {
            offset: at,
            reason: "run length out of range",
        })?;
        out.extend(core::iter::repeat_n(value, len));
        left -= len;
        value = !value;
    }
    match left {
        0 => Ok(()),
        _ => Err(CodecError::Corrupt {
            offset: at,
            reason: "runs do not cover the block",
        }),
    }
}

/// Lengths of the alternating runs.
fn runs(values: &[bool]) -> Vec<i64> {
    values
        .chunk_by(|a, b| a == b)
        .map(|c| c.len() as i64)
        .collect()
}
