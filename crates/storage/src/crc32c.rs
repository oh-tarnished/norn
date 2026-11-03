//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! CRC-32C (Castagnoli), the checksum over every checksummed region.
//!
//! Why CRC-32C rather than xxHash64 (docs/reference/storage.md "Checksums"): a CRC is built
//! for error *detection* and gives guarantees a hash does not. CRC-32C
//! detects every burst error up to 32 bits and every odd number of flipped
//! bits, and is the checksum ext4, Btrfs, iSCSI, LevelDB and RocksDB use for
//! the same job. xxHash64 is a fast hash with good distribution, but it
//! promises nothing about any particular error pattern. Its extra 32 bits
//! only lower the random-collision rate from 2^-32 to 2^-64 per region.
//! CPUs also accelerate CRC-32C in hardware (SSE4.2 `crc32`, ARMv8 `crc32c*`),
//! a later option that would need `unsafe` intrinsics and is not taken here.
//!
//! This implementation is table-driven "slicing-by-8": eight 1 KiB tables
//! built at compile time, eight bytes per step, no `unsafe` and no dependency.
//!
//! Performance: see docs/reference/storage.md "Checksums" for the measured rate, far above
//! the write path. Concurrency: pure functions over immutable tables; callable
//! from any thread.

/// The reflected Castagnoli polynomial.
const POLY: u32 = 0x82F6_3B78;

/// `TABLES[k][b]` is the CRC contribution of byte `b` followed by `k` zero
/// bytes.
static TABLES: [[u32; 256]; 8] = build();

#[allow(clippy::indexing_slicing)] // Const evaluation: any fault is a compile error.
const fn build() -> [[u32; 256]; 8] {
    let mut t = [[0u32; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut c = i as u32;
        let mut bit = 0;
        while bit < 8 {
            c = if c & 1 == 1 { (c >> 1) ^ POLY } else { c >> 1 };
            bit += 1;
        }
        t[0][i] = c;
        i += 1;
    }
    let mut k = 1;
    while k < 8 {
        let mut i = 0;
        while i < 256 {
            let prev = t[k - 1][i];
            t[k][i] = (prev >> 8) ^ t[0][(prev & 0xff) as usize];
            i += 1;
        }
        k += 1;
    }
    t
}

/// One table lookup; a `u8` index always lies inside a 256-entry table, so
/// the fallback is unreachable and the bounds check folds away.
#[inline(always)]
fn at(k: usize, b: u8) -> u32 {
    TABLES
        .get(k)
        .and_then(|t| t.get(usize::from(b)))
        .copied()
        .unwrap_or(0)
}

/// Continues a CRC-32C over `data`, starting from a previous [`crc32c`]
/// result (`0` for a fresh checksum). `crc32c(ab) == extend(crc32c(a), b)`,
/// so a region can be checksummed in pieces without buffering it whole.
#[must_use]
pub fn extend(crc: u32, data: &[u8]) -> u32 {
    let mut c = !crc;
    let (words, rest) = data.as_chunks::<8>();
    for b in words {
        let lo = u32::from_le_bytes([b[0], b[1], b[2], b[3]]) ^ c;
        c = at(7, lo as u8)
            ^ at(6, (lo >> 8) as u8)
            ^ at(5, (lo >> 16) as u8)
            ^ at(4, (lo >> 24) as u8)
            ^ at(3, b[4])
            ^ at(2, b[5])
            ^ at(1, b[6])
            ^ at(0, b[7]);
    }
    for &byte in rest {
        c = (c >> 8) ^ at(0, (c as u8) ^ byte);
    }
    !c
}

/// The CRC-32C of `data`. Borrows `data` for the call only.
#[must_use]
pub fn crc32c(data: &[u8]) -> u32 {
    extend(0, data)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_standard_check_value_matches() {
        assert_eq!(crc32c(b"123456789"), 0xE306_9283);
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c(&[0u8; 32]), 0x8A91_36AA);
    }

    #[test]
    fn extending_in_pieces_equals_one_pass() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 31 % 251) as u8).collect();
        for split in [0, 1, 7, 8, 9, 500, 999, 1000] {
            let (a, b) = data.split_at(split);
            assert_eq!(extend(crc32c(a), b), crc32c(&data));
        }
    }

    #[test]
    fn slicing_agrees_with_the_bytewise_definition() {
        let data: Vec<u8> = (0..257u32).map(|i| (i * 7 + 3) as u8).collect();
        let mut c = !0u32;
        for &byte in &data {
            c ^= u32::from(byte);
            for _ in 0..8 {
                c = if c & 1 == 1 { (c >> 1) ^ POLY } else { c >> 1 };
            }
        }
        assert_eq!(crc32c(&data), !c);
    }
}
