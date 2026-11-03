//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! CRC-32C for the log's records, bit by bit, with no table and no `unsafe`.
//!
//! norn-storage has a faster one, but the layer table keeps this crate over the
//! core alone, and a metadata record is small and rare.

const POLY: u32 = 0x82F6_3B78;

/// The CRC-32C of `bytes`. Borrows and returns a number; allocates nothing.
pub(crate) fn crc32c(bytes: &[u8]) -> u32 {
    let mut c = !0u32;
    for b in bytes {
        c ^= u32::from(*b);
        for _ in 0..8 {
            c = if c & 1 != 0 { (c >> 1) ^ POLY } else { c >> 1 };
        }
    }
    !c
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_matches_the_published_check_value() {
        assert_eq!(super::crc32c(b"123456789"), 0xE306_9283);
    }
}
