//! Checksum-verified frame body → chunk.

use std::path::Path;

use norn_codec::compress;
use norn_codec::{CodecError, boolean, bytes, enumeration, float32, float64, integer, timestamp};
use norn_core::{Quality, ValueKind};

use super::{Chunk, Column};
use crate::error::{CorruptionKind, Result, StorageError};
use crate::format::Le;
use crate::format::frame::ChunkMeta;

/// Decodes a chunk body whose CRC the caller has already verified. The body
/// is read in place; the chunk owns copies of every value.
///
/// Memory: the decoded columns (bounded by `meta.count`, which the codec
/// caps per block) plus one opened block at a time.
///
/// # Errors
/// [`StorageError::Codec`] for a block the codec refuses;
/// [`StorageError::Corrupt`] with [`CorruptionKind::Malformed`] when the
/// sections or counts are inconsistent, or a value is out of range for its
/// kind (an `i32` that does not fit, a non-UTF-8 string, an unknown quality).
pub(crate) fn decode_body(meta: &ChunkMeta, body: &[u8], path: &Path) -> Result<Chunk> {
    let malformed = |why| {
        StorageError::corrupt(
            path,
            meta.offset,
            meta.frame_len(),
            CorruptionKind::Malformed(why),
        )
    };
    let codec = |source: CodecError| StorageError::Codec {
        path: path.to_path_buf(),
        offset: meta.offset,
        source,
    };
    let mut r = Le::new(body);
    let mut block = Vec::new();
    let mut next = |block: &mut Vec<u8>| -> Result<()> {
        let len = r.u32().ok_or_else(|| malformed("section length"))?;
        let env = r
            .take(len as usize)
            .ok_or_else(|| malformed("section past body"))?;
        block.clear();
        compress::open(env, block).map(drop).map_err(codec)
    };
    let mut ts = Vec::new();
    next(&mut block)?;
    timestamp::decode(&block, &mut ts).map_err(codec)?;
    next(&mut block)?;
    let values = values(meta.kind, &block).map_err(codec)?;
    let values = values.ok_or_else(|| malformed("value out of range for its kind"))?;
    next(&mut block)?;
    let mut codes = Vec::new();
    enumeration::decode(&block, &mut codes).map_err(codec)?;
    next(&mut block)?;
    let mut seqs = Vec::new();
    integer::decode(&block, &mut seqs).map_err(codec)?;
    if r.pos() != body.len() {
        return Err(malformed("bytes after the last section"));
    }
    let n = meta.count as usize;
    if ts.len() != n || values.len() != n || codes.len() != n || seqs.len() != n {
        return Err(malformed("column lengths differ from the count"));
    }
    let qualities = codes
        .into_iter()
        .map(|c| {
            u16::try_from(c)
                .ok()
                .and_then(|c| Quality::from_code(c).ok())
        })
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| malformed("unknown quality code"))?;
    let seqs = seqs.into_iter().map(|s| s as u64).collect();
    Ok(Chunk::from_parts(meta.signal, ts, values, qualities, seqs))
}

/// Decodes the value column for `kind`; `Ok(None)` when a value does not
/// fit its kind.
fn values(kind: ValueKind, block: &[u8]) -> norn_codec::Result<Option<Column>> {
    let ints = |block: &[u8]| -> norn_codec::Result<Vec<i64>> {
        let mut v = Vec::new();
        integer::decode(block, &mut v)?;
        Ok(v)
    };
    Ok(Some(match kind {
        ValueKind::Bool => {
            let mut v = Vec::new();
            boolean::decode(block, &mut v)?;
            Column::Bool(v)
        }
        ValueKind::I32 => match ints(block)?.into_iter().map(i32::try_from).collect() {
            Ok(v) => Column::I32(v),
            Err(_) => return Ok(None),
        },
        ValueKind::I64 => Column::I64(ints(block)?),
        ValueKind::U32 => match ints(block)?.into_iter().map(u32::try_from).collect() {
            Ok(v) => Column::U32(v),
            Err(_) => return Ok(None),
        },
        ValueKind::U64 => Column::U64(ints(block)?.into_iter().map(|x| x as u64).collect()),
        ValueKind::F32 => {
            let mut v = Vec::new();
            float32::decode(block, &mut v)?;
            Column::F32(v)
        }
        ValueKind::F64 => {
            let mut v = Vec::new();
            float64::decode(block, &mut v)?;
            Column::F64(v)
        }
        ValueKind::String => {
            let mut v = Vec::new();
            bytes::decode(block, &mut v)?;
            match v.into_iter().map(String::from_utf8).collect() {
                Ok(v) => Column::String(v),
                Err(_) => return Ok(None),
            }
        }
        ValueKind::Bytes => {
            let mut v = Vec::new();
            bytes::decode(block, &mut v)?;
            Column::Bytes(v)
        }
        ValueKind::Enum => {
            let mut v = Vec::new();
            enumeration::decode(block, &mut v)?;
            Column::Enum(v)
        }
        _ => return Ok(None),
    }))
}
