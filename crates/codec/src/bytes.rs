//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Byte-string columns: length-prefixed, or through a dictionary.
//!
//! Lengths are bit-packed (frame-of-reference miniblocks), then the bytes
//! follow back to back. When values repeat (a status string, a unit, an
//! event label) the dictionary form stores each distinct value once and one
//! small index per row. [`encode`] measures both and keeps the smaller; zstd
//! on the block then handles redundancy *within* values.

use crate::dict;
use crate::error::{CodecError, Result};
use crate::header;
use crate::id::{CodecId, ColumnType};
use crate::int::pack;
use crate::limits::MAX_BLOCK_BYTES;
use crate::wire::{Cursor, put_varint};

const COLUMN: ColumnType = ColumnType::Bytes;

/// Encodes `values` with the smaller byte-string codec, appending one block.
///
/// Performance: O(total bytes) with one hash per value.
///
/// # Errors
/// [`CodecError::TooLarge`] above [`crate::MAX_BLOCK_VALUES`] values or
/// [`MAX_BLOCK_BYTES`] total bytes.
pub fn encode<B: AsRef<[u8]>>(values: &[B], out: &mut Vec<u8>) -> Result<CodecId> {
    let refs = check(values)?;
    let (d, idx) = dict::build(&refs);
    let plain: usize = pack::size(&lengths(&refs)) + refs.iter().map(|v| v.len()).sum::<usize>();
    let dict_size = pack::varint_len(d.len() as u64)
        + pack::size(&lengths(&d))
        + d.iter().map(|v| v.len()).sum::<usize>()
        + dict::indices_len(idx.len(), dict::index_width(d.len()));
    let codec = if dict_size < plain {
        CodecId::BytesDict
    } else {
        CodecId::BytesPlain
    };
    write(codec, &refs, &d, &idx, out);
    Ok(codec)
}

/// Encodes with a forced byte-string codec.
///
/// # Errors
/// As [`encode`], plus [`CodecError::WrongColumn`].
pub fn encode_with<B: AsRef<[u8]>>(codec: CodecId, values: &[B], out: &mut Vec<u8>) -> Result<()> {
    header::check_column(codec, COLUMN)?;
    let refs = check(values)?;
    let (d, idx) = dict::build(&refs);
    write(codec, &refs, &d, &idx, out);
    Ok(())
}

fn check<B: AsRef<[u8]>>(values: &[B]) -> Result<Vec<&[u8]>> {
    header::check_len(values.len())?;
    let refs: Vec<&[u8]> = values.iter().map(AsRef::as_ref).collect();
    let total: usize = refs.iter().map(|v| v.len()).fold(0, usize::saturating_add);
    if total > MAX_BLOCK_BYTES {
        return Err(CodecError::TooLarge {
            offset: 0,
            len: total as u64,
            max: MAX_BLOCK_BYTES as u64,
        });
    }
    Ok(refs)
}

fn lengths(values: &[&[u8]]) -> Vec<i64> {
    values.iter().map(|v| v.len() as i64).collect()
}

fn put_values(values: &[&[u8]], out: &mut Vec<u8>) {
    pack::encode(&lengths(values), out);
    values.iter().for_each(|v| out.extend_from_slice(v));
}

fn write(codec: CodecId, refs: &[&[u8]], d: &[&[u8]], idx: &[u32], out: &mut Vec<u8>) {
    header::write(out, codec, refs.len());
    if codec == CodecId::BytesDict {
        put_varint(out, d.len() as u64);
        put_values(d, out);
        dict::write_indices(idx, dict::index_width(d.len()), out);
    } else {
        put_values(refs, out);
    }
}

/// Reads `n` length-prefixed values, borrowing them from the block.
fn read_values<'a>(cur: &mut Cursor<'a>, n: usize) -> Result<Vec<&'a [u8]>> {
    let mut lens = Vec::with_capacity(n);
    let at = cur.pos();
    pack::decode(cur, n, &mut lens)?;
    let mut out = Vec::with_capacity(n);
    for len in lens {
        let len = usize::try_from(len).ok().filter(|l| *l <= MAX_BLOCK_BYTES);
        let len = len.ok_or(CodecError::Corrupt {
            offset: at,
            reason: "length out of range",
        })?;
        out.push(cur.take(len)?);
    }
    Ok(out)
}

/// Decodes one block, appending owned copies of its values to `out`.
///
/// Every stored length is checked against the bytes actually present before
/// copying, so total allocation never exceeds the block's own size.
///
/// # Errors
/// Any decode variant of [`CodecError`]; never panics on corrupt input.
pub fn decode(block: &[u8], out: &mut Vec<Vec<u8>>) -> Result<()> {
    let mut cur = Cursor::new(block);
    let h = header::read_for(&mut cur, COLUMN)?;
    if h.codec == CodecId::BytesDict {
        let dict_len = cur.count(h.count)?;
        let d = read_values(&mut cur, dict_len)?;
        let at = cur.pos();
        let idx = dict::read_indices(&mut cur, h.count, dict_len)?;
        // A dictionary multiplies bytes; bound the expansion, as encode did.
        let total = idx
            .iter()
            .map(|i| d.get(*i as usize).map_or(0, |v| v.len()));
        let total = total.fold(0usize, usize::saturating_add);
        if total > MAX_BLOCK_BYTES {
            let (len, max) = (total as u64, MAX_BLOCK_BYTES as u64);
            return Err(CodecError::TooLarge {
                offset: at,
                len,
                max,
            });
        }
        out.reserve(h.count);
        out.extend(
            idx.iter()
                .map(|i| d.get(*i as usize).map(|v| v.to_vec()).unwrap_or_default()),
        );
    } else {
        let values = read_values(&mut cur, h.count)?;
        out.extend(values.into_iter().map(<[u8]>::to_vec));
    }
    cur.finish()
}
