//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What every fuzz target shares (stage 16).
//!
//! # Layer contract
//!
//! - **Owns**: measuring a call's peak heap ([`measure`]), so a target can hold
//!   a decoder to the budget it claims; scratch files for decoders that read
//!   files; CRC repair, so mutations reach what a checksum guards; and the
//!   codec checks every column target runs.
//! - **Must not**: be linked by anything Norn ships. It lives in its own
//!   workspace beside Norn's, which never builds it.
//! - **May import**: any Norn crate, through its `fuzzing` feature where its
//!   decoders are crate-private.
//! - **Spans**: none.
//!
//! Every target asserts the same contract: no panic on any input, no
//! allocation beyond what the decoder's configured bound allows, and, where an
//! encoder exists, that what decodes re-encodes to the same values.

mod alloc;
pub mod column;
mod drive;
pub mod requests;
mod scratch;
pub mod segment;
pub mod source;

pub use alloc::{Counting, measure};
pub use drive::{budget, limits, runtime, settled};
pub use scratch::scratch;

/// The global allocator of every target: the system's, counted.
#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Inputs above this many bytes are skipped: libFuzzer finds the same paths
/// with smaller ones, and a target must stay fast to be useful.
pub const MAX_INPUT: usize = 1 << 20;

/// `body` with its CRC-32C appended, the trailer every Norn file ends with.
#[must_use]
pub fn with_crc(body: &[u8]) -> Vec<u8> {
    let mut out = body.to_vec();
    out.extend_from_slice(&norn_storage::crc32c(body).to_le_bytes());
    out
}

/// Splits a selector byte off `data`: `(selector, rest)`, or `None` if empty.
#[must_use]
pub fn split(data: &[u8]) -> Option<(u8, &[u8])> {
    data.split_first().map(|(s, rest)| (*s, rest))
}

/// The little-endian `u64` at `at`, or zero when `data` is too short.
#[must_use]
pub fn u64_at(data: &[u8], at: usize) -> u64 {
    data.get(at..at + 8)
        .and_then(|b| <[u8; 8]>::try_from(b).ok())
        .map_or(0, u64::from_le_bytes)
}

/// The little-endian `u32` at `at`, or zero when `data` is too short.
#[must_use]
pub fn u32_at(data: &[u8], at: usize) -> u32 {
    data.get(at..at + 4)
        .and_then(|b| <[u8; 4]>::try_from(b).ok())
        .map_or(0, u32::from_le_bytes)
}

/// `data`, whose last four bytes are replaced by its CRC-32C when `repair`: a
/// mutation then reaches what the checksum guards, not just the checksum.
#[must_use]
pub fn repaired(data: &[u8], repair: bool) -> Vec<u8> {
    match (repair, data.len().checked_sub(4)) {
        (true, Some(body)) => with_crc(data.get(..body).unwrap_or_default()),
        _ => data.to_vec(),
    }
}
