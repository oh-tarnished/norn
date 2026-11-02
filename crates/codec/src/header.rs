//! The self-describing block header shared by every codec (SPEC §40).
//!
//! Layout: `codec_id: u8`, `codec_version: u8`, `count: varint`. The payload
//! that follows is defined by the codec. A reader can therefore identify,
//! version-check and size any block without knowing its column in advance.

use crate::error::{CodecError, Result};
use crate::id::{CODEC_VERSION, CodecId, ColumnType};
use crate::limits::MAX_BLOCK_VALUES;
use crate::wire::{Cursor, put_varint};

/// The decoded header of a block.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockHeader {
    /// Which encoding the payload uses.
    pub codec: CodecId,
    /// The layout version of that encoding.
    pub codec_version: u8,
    /// How many values the block holds.
    pub count: usize,
    /// Bytes occupied by the header itself (2 to 5).
    pub len: usize,
}

/// Reads a block's header without decoding its payload. Borrows nothing
/// beyond the call; cost is O(1).
///
/// # Errors
/// [`CodecError::Truncated`], [`CodecError::UnknownCodec`],
/// [`CodecError::UnsupportedVersion`], or [`CodecError::TooLarge`] for a
/// count above [`MAX_BLOCK_VALUES`].
pub fn peek_header(block: &[u8]) -> Result<BlockHeader> {
    let mut cur = Cursor::new(block);
    read(&mut cur)
}

/// Appends a header to `out`.
pub(crate) fn write(out: &mut Vec<u8>, codec: CodecId, count: usize) {
    out.push(codec as u8);
    out.push(CODEC_VERSION);
    put_varint(out, count as u64);
}

/// Reads and validates a header.
pub(crate) fn read(cur: &mut Cursor<'_>) -> Result<BlockHeader> {
    let start = cur.pos();
    let id = cur.u8()?;
    let codec = CodecId::from_u8(id).ok_or(CodecError::UnknownCodec { offset: start, id })?;
    let version = cur.u8()?;
    if version != CODEC_VERSION {
        let offset = start + 1;
        return Err(CodecError::UnsupportedVersion {
            offset,
            codec,
            version,
        });
    }
    let count = cur.count(MAX_BLOCK_VALUES)?;
    let len = cur.pos() - start;
    Ok(BlockHeader {
        codec,
        codec_version: version,
        count,
        len,
    })
}

/// Reads a header and checks it belongs to `column`.
pub(crate) fn read_for(cur: &mut Cursor<'_>, column: ColumnType) -> Result<BlockHeader> {
    let offset = cur.pos();
    let h = read(cur)?;
    if h.codec.column() != column {
        return Err(CodecError::WrongColumn {
            offset,
            codec: h.codec,
            expected: column,
        });
    }
    Ok(h)
}

/// Rejects an encode of more than [`MAX_BLOCK_VALUES`] values.
pub(crate) fn check_len(n: usize) -> Result<()> {
    match n <= MAX_BLOCK_VALUES {
        true => Ok(()),
        false => Err(CodecError::TooLarge {
            offset: 0,
            len: n as u64,
            max: MAX_BLOCK_VALUES as u64,
        }),
    }
}

/// Rejects a forced codec from another column type.
pub(crate) fn check_column(codec: CodecId, column: ColumnType) -> Result<()> {
    match codec.column() == column {
        true => Ok(()),
        false => Err(CodecError::WrongColumn {
            offset: 0,
            codec,
            expected: column,
        }),
    }
}
