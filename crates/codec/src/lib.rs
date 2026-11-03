//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's encoding layer: lossless column codecs and block compression
//! (SPEC §14, stage 6).
//!
//! # Layer contract
//!
//! - **Owns** the byte layout of an encoded column block and of the
//!   compression envelope around it. Every block starts with a
//!   self-describing header (`codec_id`, `codec_version`, count; SPEC §40),
//!   so a reader never needs out-of-band knowledge to decode or reject it.
//! - **Must not** know about entities, signals, files, chunks or I/O. It
//!   works on primitive slices (`i64`, `f64`, `f32`, `u32`, `bool`, bytes)
//!   and returns bytes; the storage crate decides where they live.
//! - **Imports** no other Norn crate. Its only dependency is the pure-Rust
//!   `structured-zstd` encoder (no C; see [`compress`] and docs/reference/storage.md).
//! - **Opens no spans and performs no I/O**; it reads no clock.
//!
//! # Shape of the API
//!
//! One module per column type, each with `encode` (measure the candidate
//! codecs and keep the smallest), `encode_with` (force a codec) and
//! `decode`. Encoders append to a caller-owned `Vec<u8>` so buffers can be
//! reused; decoders append to a caller-owned `Vec`. Then [`compress::seal`]
//! optionally zstd-compresses the block, storing it raw when that does not
//! shrink it.
//!
//! ```
//! use norn_codec::{compress, timestamp, CodecId};
//! let ts: Vec<i64> = (0..3600).map(|i| 1_700_000_000_000 + i * 1000).collect();
//! let mut block = Vec::new();
//! assert_eq!(timestamp::encode(&ts, &mut block)?, CodecId::TimestampRegular);
//! let mut sealed = Vec::new();
//! compress::seal(&block, compress::Compression::Default, &mut sealed)?;
//! let (mut opened, mut back) = (Vec::new(), Vec::new());
//! compress::open(&sealed, &mut opened)?;
//! timestamp::decode(&opened, &mut back)?;
//! assert_eq!(back, ts);
//! # Ok::<(), norn_codec::CodecError>(())
//! ```
//!
//! # Guarantees
//!
//! - `decode(encode(x)) == x` for every codec, bit for bit (floats compare
//!   by bit pattern, so NaN payloads and `-0.0` survive).
//! - Decoding never panics and never allocates beyond [`MAX_BLOCK_VALUES`]
//!   values or [`MAX_BLOCK_BYTES`] bytes per block; malformed input yields a
//!   typed [`CodecError`] carrying the byte offset.
//! - Encoding is deterministic: the same input yields the same bytes.
//!
//! # Concurrency
//!
//! Every function is pure over its arguments and holds no global state; all
//! of them may run concurrently on different blocks from any thread.

#![forbid(unsafe_code)]

mod bits;
pub mod boolean;
pub mod bytes;
mod column;
pub mod compress;
mod dict;
pub mod enumeration;
mod error;
mod float;
mod header;
mod id;
mod int;
pub mod limits;
mod wire;
mod zstd;

pub use column::{integer, timestamp};
pub use error::{CodecError, Result};
pub use float::api::{float32, float64};
pub use header::{BlockHeader, peek_header};
pub use id::{ALL_CODECS, CODEC_VERSION, CodecId, ColumnType};
pub use limits::{MAX_BLOCK_BYTES, MAX_BLOCK_VALUES};
