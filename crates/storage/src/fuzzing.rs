//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The chunk decoder and the frame encoder, exposed for the fuzz crate only
//! (feature `fuzzing`), so mutations reach what a frame's CRC guards. Not an
//! API: no shipped build enables the feature.

use std::path::Path;

use crate::chunk::Chunk;
use crate::chunk::decode::decode_body;
use crate::crc32c::crc32c;
use crate::error::Result;
use crate::format::frame::{self, ChunkMeta};

/// Decodes a chunk body as a read does once its CRC has passed. Copies.
///
/// # Errors
/// What reading the chunk returns.
pub fn decode_chunk(meta: &ChunkMeta, body: &[u8]) -> Result<Chunk> {
    decode_body(meta, body, Path::new("fuzz"))
}

/// A frame of `body` for `meta`, whose header and CRCs are all correct.
#[must_use]
pub fn frame(meta: &ChunkMeta, body: &[u8]) -> Vec<u8> {
    let mut out = frame::encode(meta, crc32c(body)).to_vec();
    out.extend_from_slice(body);
    out
}
