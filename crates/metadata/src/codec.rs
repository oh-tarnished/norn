//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Little-endian field encoding for log records.

use norn_core::{Metadata, MetadataLimits, Name, Timestamp};

/// Appends fields to a record.
#[derive(Default)]
pub(crate) struct Out(pub(crate) Vec<u8>);

impl Out {
    pub(crate) fn u8(&mut self, v: u8) {
        self.0.push(v);
    }

    pub(crate) fn u64(&mut self, v: u64) {
        self.0.extend_from_slice(&v.to_le_bytes());
    }

    pub(crate) fn time(&mut self, t: Timestamp) {
        self.0.extend_from_slice(&t.as_nanos().to_le_bytes());
    }

    pub(crate) fn str(&mut self, s: &str) {
        let len = u32::try_from(s.len()).unwrap_or(u32::MAX);
        self.0.extend_from_slice(&len.to_le_bytes());
        self.0.extend_from_slice(s.as_bytes());
    }

    pub(crate) fn map(&mut self, m: &Metadata) {
        let len = u32::try_from(m.len()).unwrap_or(u32::MAX);
        self.0.extend_from_slice(&len.to_le_bytes());
        for (k, v) in m.iter() {
            self.str(k);
            self.str(v);
        }
    }
}

/// Reads fields from a record whose checksum has passed. Every read is bounds
/// checked, so a checksum collision or a bug fails typed rather than panicking.
/// Copies: decoded strings own their bytes and do not borrow the record.
pub(crate) struct In<'a>(pub(crate) &'a [u8]);

impl In<'_> {
    fn take(&mut self, n: usize) -> Option<&[u8]> {
        let (head, rest) = self.0.split_at_checked(n)?;
        self.0 = rest;
        Some(head)
    }

    pub(crate) fn u8(&mut self) -> Option<u8> {
        self.take(1)?.first().copied()
    }

    pub(crate) fn u64(&mut self) -> Option<u64> {
        Some(u64::from_le_bytes(self.take(8)?.try_into().ok()?))
    }

    pub(crate) fn time(&mut self) -> Option<Timestamp> {
        let nanos = i64::from_le_bytes(self.take(8)?.try_into().ok()?);
        Some(Timestamp::from_nanos(nanos))
    }

    fn u32(&mut self) -> Option<u32> {
        Some(u32::from_le_bytes(self.take(4)?.try_into().ok()?))
    }

    pub(crate) fn str(&mut self) -> Option<String> {
        let n = usize::try_from(self.u32()?).ok()?;
        String::from_utf8(self.take(n)?.to_vec()).ok()
    }

    pub(crate) fn name(&mut self) -> Option<Name> {
        Name::new(self.str()?).ok()
    }

    /// A map as it was stored. It carries limits wide enough to hold it: a
    /// lowered configuration applies to new values, and never makes a record
    /// already acknowledged unreadable.
    pub(crate) fn map(&mut self) -> Option<Metadata> {
        let wide = MetadataLimits {
            max_entries: usize::MAX,
            max_key_bytes: usize::MAX,
            max_value_bytes: usize::MAX,
        };
        let mut m = Metadata::with_limits(wide);
        for _ in 0..self.u32()? {
            let (k, v) = (self.str()?, self.str()?);
            m.insert(k, v).ok()?;
        }
        Some(m)
    }
}
