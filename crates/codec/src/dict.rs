//! Dictionary building and index packing, shared by enum and byte-string
//! columns. Internal to the crate.
//!
//! The dictionary lists distinct values in order of first appearance, so the
//! encoding is deterministic for a given input (no hash-order dependence).
//! Its size is bounded by the block's value count, itself bounded by
//! [`crate::MAX_BLOCK_VALUES`].

use std::collections::HashMap;
use std::hash::Hash;

use crate::bits::{BitReader, BitWriter, packed_len, width_of};
use crate::error::{CodecError, Result};
use crate::wire::Cursor;

/// Distinct values in first-appearance order, and each value's index.
pub(crate) fn build<K: Hash + Eq + Copy>(values: &[K]) -> (Vec<K>, Vec<u32>) {
    let mut map: HashMap<K, u32> = HashMap::new();
    let mut dict = Vec::new();
    let idx = values
        .iter()
        .map(|v| {
            *map.entry(*v).or_insert_with(|| {
                dict.push(*v);
                (dict.len() - 1) as u32
            })
        })
        .collect();
    (dict, idx)
}

/// Bits per index for a dictionary of `len` entries (0 when `len <= 1`).
pub(crate) fn index_width(len: usize) -> u32 {
    width_of(len.saturating_sub(1) as u64)
}

/// Appends `idx` packed at `width` bits each.
pub(crate) fn write_indices(idx: &[u32], width: u32, out: &mut Vec<u8>) {
    let mut w = BitWriter::new(out);
    idx.iter().for_each(|i| w.write(u64::from(*i), width));
    w.finish();
}

/// Bytes [`write_indices`] produces.
pub(crate) fn indices_len(n: usize, width: u32) -> usize {
    packed_len(n, width).unwrap_or(usize::MAX)
}

/// Reads `n` indices for a dictionary of `dict_len` entries, rejecting any
/// index outside it.
pub(crate) fn read_indices(cur: &mut Cursor<'_>, n: usize, dict_len: usize) -> Result<Vec<u32>> {
    let width = index_width(dict_len);
    let base = cur.pos();
    let mut r = BitReader::new(cur.take(indices_len(n, width))?, base);
    let mut out = Vec::with_capacity(n);
    for _ in 0..n {
        let i = r.read(width)?;
        if i >= dict_len as u64 {
            let offset = base + r.consumed().saturating_sub(1);
            return Err(CodecError::Corrupt {
                offset,
                reason: "index outside the dictionary",
            });
        }
        out.push(i as u32);
    }
    Ok(out)
}
