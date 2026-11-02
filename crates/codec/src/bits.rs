//! LSB-first bit packing. Internal to the crate.
//!
//! Values are written least-significant bit first into consecutive bytes;
//! the final partial byte is zero-padded. A 128-bit accumulator lets one call
//! write any width from 0 to 64 bits without branching on byte boundaries.

use crate::error::{CodecError, Result};

/// The low `width` bits set, for `width` in 0..=64.
pub(crate) fn mask(width: u32) -> u64 {
    match width {
        0 => 0,
        64.. => u64::MAX,
        w => (1u64 << w) - 1,
    }
}

/// The number of bits needed to represent `v` (0 for 0).
pub(crate) fn width_of(v: u64) -> u32 {
    64 - v.leading_zeros()
}

/// Appends packed bits to a byte vector.
pub(crate) struct BitWriter<'a> {
    out: &'a mut Vec<u8>,
    acc: u128,
    n: u32,
}

impl<'a> BitWriter<'a> {
    /// Starts packing at the end of `out`.
    pub(crate) fn new(out: &'a mut Vec<u8>) -> Self {
        Self { out, acc: 0, n: 0 }
    }

    /// Writes the low `width` bits of `v` (`width` at most 64).
    pub(crate) fn write(&mut self, v: u64, width: u32) {
        let width = width.min(64);
        self.acc |= u128::from(v & mask(width)) << self.n;
        self.n += width;
        while self.n >= 8 {
            self.out.push(self.acc as u8);
            self.acc >>= 8;
            self.n -= 8;
        }
    }

    /// Writes one bit.
    pub(crate) fn bit(&mut self, b: bool) {
        self.write(u64::from(b), 1);
    }

    /// Flushes the final partial byte.
    pub(crate) fn finish(self) {
        if self.n > 0 {
            self.out.push(self.acc as u8);
        }
    }
}

/// Reads packed bits from a borrowed slice; never reads past its end.
pub(crate) struct BitReader<'a> {
    buf: &'a [u8],
    /// Offset of `buf` within the whole block, for error reporting.
    base: usize,
    byte: usize,
    acc: u128,
    n: u32,
}

impl<'a> BitReader<'a> {
    /// Reads `buf`, which starts at byte `base` of the block.
    pub(crate) fn new(buf: &'a [u8], base: usize) -> Self {
        Self {
            buf,
            base,
            byte: 0,
            acc: 0,
            n: 0,
        }
    }

    /// Reads `width` bits (at most 64).
    pub(crate) fn read(&mut self, width: u32) -> Result<u64> {
        let width = width.min(64);
        while self.n < width {
            let Some(b) = self.buf.get(self.byte).copied() else {
                return Err(CodecError::Truncated {
                    offset: self.base + self.byte,
                    needed: 1,
                });
            };
            self.acc |= u128::from(b) << self.n;
            self.byte += 1;
            self.n += 8;
        }
        let v = (self.acc as u64) & mask(width);
        self.acc >>= width;
        self.n -= width;
        Ok(v)
    }

    /// Reads one bit.
    pub(crate) fn bit(&mut self) -> Result<bool> {
        self.read(1).map(|b| b == 1)
    }

    /// Bytes of `buf` consumed so far (including a partially read byte).
    pub(crate) fn consumed(&self) -> usize {
        self.byte
    }
}

/// Bytes needed to pack `count` values of `width` bits.
pub(crate) fn packed_len(count: usize, width: u32) -> Option<usize> {
    count
        .checked_mul(width as usize)
        .map(|bits| bits.div_ceil(8))
}
