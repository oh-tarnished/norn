//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Declared bounds on block size (SPEC §16: no unbounded buffers).
//!
//! Encoders refuse inputs above these limits and decoders check every stored
//! count against them *before* allocating, so a corrupted or hostile length
//! field can never make the decoder reserve more than these bounds allow.

/// Maximum values in one block: 2^20 (1,048,576), about 12 days of a 1 Hz
/// signal. A decoded `f64`/`i64` block is therefore at most 8 MiB.
pub const MAX_BLOCK_VALUES: usize = 1 << 20;

/// Maximum bytes in one encoded block before compression, and the maximum
/// total payload of a byte-string column: 16 MiB. This also caps the zstd
/// window the decoder will accept, bounding decompression memory.
pub const MAX_BLOCK_BYTES: usize = 16 << 20;

/// Values per bit-packing miniblock. Each miniblock picks its own bit width,
/// so one outlier (a timestamp gap, a counter reset) widens only its own
/// 128 values rather than the whole block.
pub const MINIBLOCK: usize = 128;
