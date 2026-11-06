//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One segment's AEAD: its key, and sealing and opening its frames.

use std::path::Path;

use chacha20poly1305::aead::{AeadInOut, KeyInit};
use chacha20poly1305::{ChaCha20Poly1305, Nonce, Tag};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::Zeroizing;

use super::error::EncryptionError;
use super::keyring::MasterKey;
use super::{EncryptionInfo, KEY_LEN};
use crate::error::{CorruptionKind, Result, StorageError};
use crate::format::frame::{self, ChunkMeta};
use crate::format::{ENCRYPTED_FORMAT_VERSION, FRAME_HEADER_LEN, SegmentHeader};
use crate::options::StorageOptions;

/// Bytes the AEAD tag adds to the end of every encrypted chunk body.
pub const TAG_LEN: usize = 16;

/// Frame header bytes bound as associated data: all but the magic and the CRCs.
const BOUND: std::ops::Range<usize> = 4..40;
/// Associated data: version 2, stream 8, `first_seq` 8, generation 2, offset 8,
/// frame header 36.
const AAD_LEN: usize = 64;

/// The AEAD of one segment file. Holds a derived key, so nothing else needs the
/// master key to read or write this file.
///
/// Concurrency: read-only once built. Memory: a key schedule, zeroed on drop.
pub(crate) struct SegmentCipher {
    aead: ChaCha20Poly1305,
    /// The associated data's first 20 bytes: what names the segment.
    segment: [u8; 20],
}

impl std::fmt::Debug for SegmentCipher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SegmentCipher(..)")
    }
}

impl SegmentCipher {
    /// The cipher of `header`'s segment, keyed from `master` and `info`.
    pub(crate) fn derive(
        master: &MasterKey,
        header: &SegmentHeader,
        info: &EncryptionInfo,
    ) -> Result<Self> {
        let mut named = [0u8; 20];
        named[..2].copy_from_slice(&ENCRYPTED_FORMAT_VERSION.to_le_bytes());
        named[2..10].copy_from_slice(&header.stream.get().to_le_bytes());
        named[10..18].copy_from_slice(&header.first_seq.to_le_bytes());
        named[18..20].copy_from_slice(&header.generation.to_le_bytes());
        let mut context = Vec::with_capacity(37);
        context.extend_from_slice(b"norn segment key v1");
        context.extend_from_slice(&named[2..]);
        let mut key = Zeroizing::new([0u8; KEY_LEN]);
        Hkdf::<Sha256>::new(Some(&info.salt), master.bytes.as_slice())
            .expand(&context, key.as_mut_slice())
            .map_err(|_| StorageError::Encryption(EncryptionError::Random))?;
        let aead = ChaCha20Poly1305::new_from_slice(key.as_slice())
            .map_err(|_| StorageError::Encryption(EncryptionError::Random))?;
        Ok(Self {
            aead,
            segment: named,
        })
    }

    /// The cipher of a new segment under `opts`' current key, recording its fresh
    /// salt in `header`. `None` and no change for a plain store.
    pub(crate) fn for_new(
        header: &mut SegmentHeader,
        opts: &StorageOptions,
    ) -> Result<Option<Self>> {
        let Some(keys) = opts.keyring else {
            return Ok(None);
        };
        let random = StorageError::Encryption(EncryptionError::Random);
        let master = keys.current_key().ok_or(random)?;
        let info = EncryptionInfo::fresh(master.id).map_err(StorageError::Encryption)?;
        header.encryption = Some(info);
        Self::derive(master, header, &info).map(Some)
    }

    /// The nonce at file offset `offset`. Offsets strictly increase within a
    /// file, each file is written once, and each has its own key.
    fn nonce(offset: u64) -> Nonce {
        let mut n = [0u8; 12];
        n[..8].copy_from_slice(&offset.to_le_bytes());
        Nonce::from(n)
    }

    fn aad(&self, offset: u64, header: &[u8]) -> [u8; AAD_LEN] {
        let mut aad = [0u8; AAD_LEN];
        aad[..20].copy_from_slice(&self.segment);
        aad[20..28].copy_from_slice(&offset.to_le_bytes());
        if let Some(bound) = header.get(BOUND) {
            aad[28..].copy_from_slice(bound);
        }
        aad
    }

    /// Turns the plaintext frame at `frame[start..]` into its encrypted form at
    /// file offset `offset`: the body encrypted in place, the tag appended, the
    /// header rewritten with the longer length and the new CRC. Returns the
    /// frame's metadata.
    pub(crate) fn seal_frame(
        &self,
        frame: &mut Vec<u8>,
        start: usize,
        meta: &ChunkMeta,
        offset: u64,
    ) -> Result<ChunkMeta> {
        let body_at = start + FRAME_HEADER_LEN;
        let plain = frame.len().saturating_sub(body_at);
        let body_len = u32::try_from(plain + TAG_LEN).unwrap_or(u32::MAX);
        let sealed = ChunkMeta {
            offset,
            body_len,
            ..*meta
        };
        let aad = self.aad(offset, &frame::encode(&sealed, 0));
        let body = frame.get_mut(body_at..).unwrap_or_default();
        let tag = self
            .aead
            .encrypt_inout_detached(&Self::nonce(offset), &aad, body.into())
            .map_err(|_| StorageError::InvalidChunk("a chunk too long to encrypt"))?;
        frame.extend_from_slice(tag.as_slice());
        let crc = crate::crc32c::crc32c(frame.get(body_at..).unwrap_or_default());
        let header = frame::encode(&sealed, crc);
        if let Some(slot) = frame.get_mut(start..body_at) {
            slot.copy_from_slice(&header);
        }
        Ok(sealed)
    }

    /// Authenticates and decrypts `frame`, one whole verified frame, in place,
    /// and cuts the tag off, leaving the plaintext body after the header.
    ///
    /// # Errors
    /// [`StorageError::Corrupt`] with [`CorruptionKind::Authentication`], and
    /// `frame` holds no usable body.
    pub(crate) fn open_frame(
        &self,
        frame: &mut Vec<u8>,
        meta: &ChunkMeta,
        path: &Path,
    ) -> Result<()> {
        let bad = || {
            StorageError::corrupt(
                path,
                meta.offset,
                meta.frame_len(),
                CorruptionKind::Authentication,
            )
        };
        let at = frame
            .len()
            .checked_sub(TAG_LEN)
            .filter(|at| *at >= FRAME_HEADER_LEN)
            .ok_or_else(bad)?;
        let aad = self.aad(
            meta.offset,
            frame.get(..FRAME_HEADER_LEN).unwrap_or_default(),
        );
        let (head, tag) = frame.split_at_mut(at);
        let tag = Tag::try_from(&*tag).map_err(|_| bad())?;
        let body = head.get_mut(FRAME_HEADER_LEN..).unwrap_or_default();
        let nonce = Self::nonce(meta.offset);
        self.aead
            .decrypt_inout_detached(&nonce, &aad, body.into(), &tag)
            .map_err(|_| bad())?;
        frame.truncate(at);
        Ok(())
    }
}
