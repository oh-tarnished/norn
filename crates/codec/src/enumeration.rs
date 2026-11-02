//! Enum columns (`u32` codes): dictionary, then bit-packed or run-length
//! indices.
//!
//! A multi-state point (occupancy mode, fan speed stage) has a few distinct
//! codes that hold for long stretches. The dictionary maps them to indices of
//! `ceil(log2(distinct))` bits; RLE then stores each stretch as one index and
//! one length. [`encode`] measures both and keeps the smaller.

use crate::dict;
use crate::error::{CodecError, Result};
use crate::header;
use crate::id::{CodecId, ColumnType};
use crate::int::pack;
use crate::wire::{Cursor, put_varint};

const COLUMN: ColumnType = ColumnType::Enum;

/// Encodes `values` with the smaller enum codec, appending one block.
///
/// Performance: O(n) with one hash lookup per value; the dictionary is at
/// most the number of distinct codes.
///
/// # Errors
/// [`CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`].
pub fn encode(values: &[u32], out: &mut Vec<u8>) -> Result<CodecId> {
    header::check_len(values.len())?;
    let (d, idx) = dict::build(values);
    let width = dict::index_width(d.len());
    let (run_idx, run_len) = runs(&idx);
    let plain = dict::indices_len(idx.len(), width);
    let rle = pack::varint_len(run_idx.len() as u64)
        + dict::indices_len(run_idx.len(), width)
        + pack::size(&run_len);
    let codec = if rle < plain {
        CodecId::EnumDictRle
    } else {
        CodecId::EnumDict
    };
    write(codec, values.len(), &d, &idx, out);
    Ok(codec)
}

/// Encodes with a forced enum codec.
///
/// # Errors
/// [`CodecError::TooLarge`], or [`CodecError::WrongColumn`].
pub fn encode_with(codec: CodecId, values: &[u32], out: &mut Vec<u8>) -> Result<()> {
    header::check_len(values.len())?;
    header::check_column(codec, COLUMN)?;
    let (d, idx) = dict::build(values);
    write(codec, values.len(), &d, &idx, out);
    Ok(())
}

fn write(codec: CodecId, n: usize, d: &[u32], idx: &[u32], out: &mut Vec<u8>) {
    header::write(out, codec, n);
    put_varint(out, d.len() as u64);
    pack::encode(&d.iter().map(|c| i64::from(*c)).collect::<Vec<_>>(), out);
    let width = dict::index_width(d.len());
    if codec == CodecId::EnumDictRle {
        let (run_idx, run_len) = runs(idx);
        put_varint(out, run_idx.len() as u64);
        dict::write_indices(&run_idx, width, out);
        pack::encode(&run_len, out);
    } else {
        dict::write_indices(idx, width, out);
    }
}

/// Decodes one block, appending to `out`; copies, borrows nothing.
///
/// # Errors
/// Any decode variant of [`CodecError`]; never panics on corrupt input.
pub fn decode(block: &[u8], out: &mut Vec<u32>) -> Result<()> {
    let mut cur = Cursor::new(block);
    let h = header::read_for(&mut cur, COLUMN)?;
    let dict_len = cur.count(h.count)?;
    let at = cur.pos();
    let mut wide = Vec::with_capacity(dict_len);
    pack::decode(&mut cur, dict_len, &mut wide)?;
    let mut d = Vec::with_capacity(dict_len);
    for c in wide {
        let c = u32::try_from(c).map_err(|_| CodecError::Corrupt {
            offset: at,
            reason: "code exceeds u32",
        })?;
        d.push(c);
    }
    out.reserve(h.count);
    if h.codec == CodecId::EnumDictRle {
        let nruns = cur.count(h.count)?;
        let run_idx = dict::read_indices(&mut cur, nruns, dict_len)?;
        let at = cur.pos();
        let mut lens = Vec::with_capacity(nruns);
        pack::decode(&mut cur, nruns, &mut lens)?;
        let mut left = h.count;
        for (i, len) in run_idx.into_iter().zip(lens) {
            let len = usize::try_from(len).ok().filter(|l| *l >= 1 && *l <= left);
            let len = len.ok_or(CodecError::Corrupt {
                offset: at,
                reason: "run length out of range",
            })?;
            let code = d.get(i as usize).copied().unwrap_or_default();
            out.extend(core::iter::repeat_n(code, len));
            left -= len;
        }
        if left != 0 {
            return Err(CodecError::Corrupt {
                offset: at,
                reason: "runs do not cover the block",
            });
        }
    } else {
        let idx = dict::read_indices(&mut cur, h.count, dict_len)?;
        out.extend(
            idx.iter()
                .map(|i| d.get(*i as usize).copied().unwrap_or_default()),
        );
    }
    cur.finish()
}

/// Run indices and lengths over dictionary indices.
fn runs(idx: &[u32]) -> (Vec<u32>, Vec<i64>) {
    let chunks = idx.chunk_by(|a, b| a == b);
    chunks
        .map(|c| (c.first().copied().unwrap_or(0), c.len() as i64))
        .unzip()
}
