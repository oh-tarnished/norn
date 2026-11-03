//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The pure-Rust zstd backend: `structured-zstd`, never the C library.
//!
//! Why this crate (SPEC §14 decision; evidence in docs/reference/storage.md): it is the one
//! pure-Rust encoder whose levels 3-6 follow upstream zstd's parameter table
//! (dfast, greedy, lazy, lazy2/btlazy2), producing output within 0.6% of the
//! C library's size at the same level, as standard frames the reference
//! `zstd` decodes. `ruzstd` panics (`unimplemented!`) above its fastest
//! level; `oxiarc-zstd` collapses levels 4-6 into one configuration.

use structured_zstd::decoding::FrameDecoder;
use structured_zstd::encoding::{CompressionLevel, compress_slice_to_vec};

use crate::error::{CodecError, Result};
use crate::limits::MAX_BLOCK_BYTES;

/// Compresses `src` into one standard zstd frame at `level`.
///
/// No content checksum is written: the storage layer checksums every chunk,
/// and a second hash would cost four bytes per block for no new guarantee.
pub(crate) fn compress(src: &[u8], level: u8) -> Vec<u8> {
    compress_slice_to_vec(src, CompressionLevel::Level(i32::from(level)))
}

/// Decompresses one frame that must expand to exactly `len` bytes.
///
/// Memory is bounded twice over: the output buffer is exactly `len` (itself
/// at most [`MAX_BLOCK_BYTES`]), and the decoder refuses any frame whose
/// window exceeds [`MAX_BLOCK_BYTES`], so a hostile header cannot make it
/// reserve a 2 GiB window.
pub(crate) fn decompress(frame: &[u8], len: usize, offset: usize) -> Result<Vec<u8>> {
    let fail = |reason| CodecError::Decompress { offset, reason };
    let mut dec = FrameDecoder::new();
    dec.set_max_window_size(MAX_BLOCK_BYTES as u64)
        .map_err(|_| fail("window ceiling rejected"))?;
    let mut out = vec![0u8; len];
    let n = dec
        .decode_all(frame, &mut out)
        .map_err(|_| fail("invalid zstd frame"))?;
    match n == len {
        true => Ok(out),
        false => Err(fail("frame length differs from recorded length")),
    }
}
