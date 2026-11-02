//! Byte-level reading and writing: a bounds-checked cursor, LEB128 varints
//! and zigzag. Internal to the crate.

use crate::error::{CodecError, Result};

/// Zigzag-maps a signed value so small magnitudes become small unsigned ones.
pub(crate) fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// Inverse of [`zigzag`].
pub(crate) fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// Appends `v` as an unsigned LEB128 varint (1 to 10 bytes).
pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Appends `v` zigzagged as a varint.
pub(crate) fn put_ivarint(out: &mut Vec<u8>, v: i64) {
    put_varint(out, zigzag(v));
}

/// A forward reader over an encoded block that never reads out of bounds.
/// Every error carries the absolute offset within the original block.
pub(crate) struct Cursor<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Starts reading at the beginning of `buf`.
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// The offset of the next unread byte.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }

    /// Bytes not yet consumed.
    pub(crate) fn remaining(&self) -> usize {
        self.buf.len().saturating_sub(self.pos)
    }

    /// The unconsumed bytes, borrowed, without consuming them. Bit-level
    /// decoders read from this, then [`Cursor::take`] what they consumed.
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.buf.get(self.pos..).unwrap_or_default()
    }

    /// Consumes and returns the next `n` bytes, borrowing from the block.
    pub(crate) fn take(&mut self, n: usize) -> Result<&'a [u8]> {
        let end = self.pos.checked_add(n);
        match end.and_then(|end| self.buf.get(self.pos..end)) {
            Some(s) => {
                self.pos += n;
                Ok(s)
            }
            None => Err(CodecError::Truncated {
                offset: self.pos,
                needed: n,
            }),
        }
    }

    /// Consumes one byte.
    pub(crate) fn u8(&mut self) -> Result<u8> {
        let b = self.buf.get(self.pos).copied();
        let b = b.ok_or(CodecError::Truncated {
            offset: self.pos,
            needed: 1,
        })?;
        self.pos += 1;
        Ok(b)
    }

    /// Consumes an unsigned LEB128 varint of at most 10 bytes.
    pub(crate) fn varint(&mut self) -> Result<u64> {
        let start = self.pos;
        let mut v: u64 = 0;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            if shift == 63 && b > 1 {
                return Err(CodecError::Corrupt {
                    offset: start,
                    reason: "varint overflows u64",
                });
            }
            v |= u64::from(b & 0x7f) << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
        }
        Err(CodecError::Corrupt {
            offset: start,
            reason: "varint longer than 10 bytes",
        })
    }

    /// Consumes a zigzag varint.
    pub(crate) fn ivarint(&mut self) -> Result<i64> {
        self.varint().map(unzigzag)
    }

    /// Consumes a varint that is a count, checking it against `max` before
    /// the caller allocates anything for it.
    pub(crate) fn count(&mut self, max: usize) -> Result<usize> {
        let offset = self.pos;
        let n = self.varint()?;
        match usize::try_from(n) {
            Ok(n) if n <= max => Ok(n),
            _ => Err(CodecError::TooLarge {
                offset,
                len: n,
                max: max as u64,
            }),
        }
    }

    /// Consumes a little-endian `u64` stored in `width` bytes (1 to 8).
    pub(crate) fn uint_le(&mut self, width: usize) -> Result<u64> {
        let bytes = self.take(width)?;
        Ok(bytes
            .iter()
            .rev()
            .fold(0u64, |acc, b| (acc << 8) | u64::from(*b)))
    }

    /// Succeeds only if every byte was consumed.
    pub(crate) fn finish(&self) -> Result<()> {
        match self.remaining() {
            0 => Ok(()),
            _ => Err(CodecError::TrailingBytes { offset: self.pos }),
        }
    }
}
