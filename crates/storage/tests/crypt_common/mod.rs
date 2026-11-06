//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Keys, options and a forger who knows every checksum but no key.
#![allow(dead_code, clippy::unwrap_used, clippy::indexing_slicing)]

use std::path::Path;

use norn_core::Sample;
use norn_storage::format::frame::ChunkMeta;
use norn_storage::{Chunk, Keyring, StorageOptions, crc32c};

pub const A: &str = "aa00000000000000000000000000000000000000000000000000000000000000";
pub const B: &str = "bb00000000000000000000000000000000000000000000000000000000000000";

/// Every sample of `chunk`.
pub fn all(chunk: &Chunk) -> Vec<Sample> {
    (0..chunk.len()).filter_map(|i| chunk.sample(i)).collect()
}

/// Options encrypting under `text`, a key file's contents.
pub fn opts(text: &str) -> StorageOptions {
    let mut o = StorageOptions::default();
    o.keyring = Some(Keyring::parse(text).unwrap().leak());
    o
}

/// Applies `change` to the frame at `meta` in `path`, then repairs its CRCs, so
/// only authentication is left to notice.
pub fn forge(path: &Path, meta: &ChunkMeta, change: impl FnOnce(&mut [u8])) {
    let mut file = std::fs::read(path).unwrap();
    let at = meta.offset as usize;
    let frame = &mut file[at..at + meta.frame_len() as usize];
    change(frame);
    recrc(frame);
    std::fs::write(path, file).unwrap();
}

/// Makes `frame`'s two CRCs right for its bytes.
pub fn recrc(frame: &mut [u8]) {
    let body = crc32c(&frame[48..]);
    frame[40..44].copy_from_slice(&body.to_le_bytes());
    let head = crc32c(&frame[..44]);
    frame[44..48].copy_from_slice(&head.to_le_bytes());
}
