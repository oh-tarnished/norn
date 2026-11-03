//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Little-endian and varint primitives for the index's own files.
//!
//! Decoding copies scalars out of the input and borrows nothing past the call,
//! except [`Cursor::rest`], whose result borrows the input.

/// Appends `v` as a LEB128 varint: at most ten bytes.
pub(crate) fn put_varint(out: &mut Vec<u8>, mut v: u64) {
    while v >= 0x80 {
        out.push((v as u8 & 0x7f) | 0x80);
        v >>= 7;
    }
    out.push(v as u8);
}

/// Maps small magnitudes to small unsigned values, so deltas of either sign
/// stay short.
pub(crate) fn zigzag(v: i64) -> u64 {
    ((v << 1) ^ (v >> 63)) as u64
}

/// The inverse of [`zigzag`].
pub(crate) fn unzigzag(v: u64) -> i64 {
    ((v >> 1) as i64) ^ -((v & 1) as i64)
}

/// A bounds-checked reader over bytes; every read returns `None` when short.
#[derive(Debug)]
pub(crate) struct Cursor<'a> {
    rest: &'a [u8],
}

impl<'a> Cursor<'a> {
    /// A cursor at the start of `bytes`.
    pub(crate) fn new(bytes: &'a [u8]) -> Self {
        Self { rest: bytes }
    }

    fn array<const N: usize>(&mut self) -> Option<[u8; N]> {
        let (head, rest) = self.rest.split_first_chunk::<N>()?;
        self.rest = rest;
        Some(*head)
    }

    pub(crate) fn magic(&mut self, expected: &[u8; 8]) -> Option<()> {
        (self.array::<8>()? == *expected).then_some(())
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.array::<1>().map(|[b]| b)
    }

    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.array().map(u16::from_le_bytes)
    }

    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.array().map(u32::from_le_bytes)
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.array().map(u64::from_le_bytes)
    }

    pub(crate) fn i64(&mut self) -> Option<i64> {
        self.array().map(i64::from_le_bytes)
    }

    /// A LEB128 varint; `None` when short or longer than a `u64`.
    pub(crate) fn varint(&mut self) -> Option<u64> {
        let mut v = 0u64;
        for shift in (0..64).step_by(7) {
            let b = self.u8()?;
            let bits = u64::from(b & 0x7f);
            if shift == 63 && bits > 1 {
                return None;
            }
            v |= bits << shift;
            if b & 0x80 == 0 {
                return Some(v);
            }
        }
        None
    }

    /// Whatever has not been read.
    pub(crate) fn rest(&self) -> &'a [u8] {
        self.rest
    }
}

/// Splits a trailing CRC-32C off `bytes` and checks it; `None` on a mismatch or
/// when too short. The result borrows `bytes`.
pub(crate) fn checked_body(bytes: &[u8]) -> Option<&[u8]> {
    let (body, crc) = bytes.split_last_chunk::<4>()?;
    (u32::from_le_bytes(*crc) == norn_storage::crc32c(body)).then_some(body)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_varint_round_trips_every_magnitude() {
        for v in [
            0,
            1,
            127,
            128,
            16_383,
            16_384,
            u64::from(u32::MAX),
            u64::MAX,
        ] {
            let mut out = Vec::new();
            put_varint(&mut out, v);
            assert_eq!(Cursor::new(&out).varint(), Some(v));
        }
    }

    #[test]
    fn zigzag_round_trips_the_extremes() {
        for v in [0, -1, 1, i64::MIN, i64::MAX] {
            assert_eq!(unzigzag(zigzag(v)), v);
        }
    }

    #[test]
    fn a_varint_longer_than_a_u64_is_refused() {
        assert_eq!(Cursor::new(&[0xff; 11]).varint(), None);
        let over = [0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x02];
        assert_eq!(Cursor::new(&over).varint(), None);
    }

    #[test]
    fn a_short_input_reads_as_none_rather_than_panicking() {
        assert_eq!(Cursor::new(&[1, 2]).u32(), None);
        assert_eq!(checked_body(&[1, 2]), None);
    }
}
