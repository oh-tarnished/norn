//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Per-segment chunk detail: the sidecar format and its cached reads.
//!
//! docs/reference/storage.md has the byte table. Chunks sort by `(signal, min_ts, offset)` and
//! sit in pages of [`PAGE_ENTRIES`], so a lookup costs one head (cached) and the
//! pages that can hold the signal, instead of the whole 704 KB directory.

mod decode;
mod encode;
mod store;

pub(crate) use decode::{DetailHead, decode_head, decode_page, head_len, page_count};
pub(crate) use encode::encode_detail;
pub(crate) use store::{read_head, read_page, write_detail};

/// A sidecar's file extension.
pub(crate) const DETAIL_EXT: &str = "ndet";
/// Chunks per page. Picked: 128 decoded metas are about 6 KiB, one or two OS
/// pages of encoded bytes. Larger pages cost every lookup more bytes; smaller
/// ones cost more fences in the head.
pub(crate) const PAGE_ENTRIES: usize = 128;
pub(crate) const MAGIC: &[u8; 8] = b"NORNDET\0";
pub(crate) const VERSION: u16 = 1;
/// Fixed bytes of a head, before the fences.
pub(crate) const FIXED_HEAD: usize = 48;
/// Bytes of one encoded fence.
pub(crate) const FENCE_LEN: usize = 12;
