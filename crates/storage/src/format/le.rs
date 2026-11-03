//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A bounds-checked little-endian reader over a fixed record.

/// Reads little-endian fields front to back. Every read returns `None`
/// past the end instead of panicking; callers check the record length first
/// so `None` only appears on a caller bug, which they map to corruption.
pub(crate) struct Le<'a> {
    buf: &'a [u8],
    pos: usize,
}

impl<'a> Le<'a> {
    /// Starts at the front of `buf`, borrowing it.
    pub(crate) fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    /// The next `N` bytes.
    pub(crate) fn bytes<const N: usize>(&mut self) -> Option<[u8; N]> {
        let end = self.pos.checked_add(N)?;
        let out = <[u8; N]>::try_from(self.buf.get(self.pos..end)?).ok()?;
        self.pos = end;
        Some(out)
    }

    /// The next byte.
    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.bytes::<1>().map(|[b]| b)
    }

    /// The next `u16`.
    pub(crate) fn u16(&mut self) -> Option<u16> {
        self.bytes().map(u16::from_le_bytes)
    }

    /// The next `u32`.
    pub(crate) fn u32(&mut self) -> Option<u32> {
        self.bytes().map(u32::from_le_bytes)
    }

    /// The next `u64`.
    pub(crate) fn u64(&mut self) -> Option<u64> {
        self.bytes().map(u64::from_le_bytes)
    }

    /// The next `i64`.
    pub(crate) fn i64(&mut self) -> Option<i64> {
        self.bytes().map(i64::from_le_bytes)
    }

    /// The next `n` bytes, borrowed.
    pub(crate) fn take(&mut self, n: usize) -> Option<&'a [u8]> {
        let end = self.pos.checked_add(n)?;
        let out = self.buf.get(self.pos..end)?;
        self.pos = end;
        Some(out)
    }

    /// Bytes consumed so far.
    pub(crate) fn pos(&self) -> usize {
        self.pos
    }
}
