//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which cipher reads a segment, if any.

use std::path::Path;

use super::{EncryptionError, SegmentCipher};
use crate::error::{Result, StorageError};
use crate::format::SegmentHeader;
use crate::options::StorageOptions;

/// The cipher to read `header`'s segment at `path` with, under `opts`; `None`
/// for a plain segment in a plain store. Reads nothing.
///
/// # Errors
/// [`EncryptionError::Mismatch`] when the segment's encryption is not the
/// configuration's; [`EncryptionError::KeyUnavailable`] for a key not held.
pub(crate) fn cipher_for(
    header: &SegmentHeader,
    opts: &StorageOptions,
    path: &Path,
) -> Result<Option<SegmentCipher>> {
    let refuse = |e| Err(StorageError::Encryption(e));
    let path = path.to_path_buf();
    match (&header.encryption, opts.keyring) {
        (None, None) => Ok(None),
        (None, Some(_)) => refuse(EncryptionError::Mismatch {
            path,
            encrypted: false,
        }),
        (Some(_), None) => refuse(EncryptionError::Mismatch {
            path,
            encrypted: true,
        }),
        (Some(info), Some(keys)) => match keys.get(info.key) {
            Some(master) => SegmentCipher::derive(master, header, info).map(Some),
            None => refuse(EncryptionError::KeyUnavailable {
                path,
                key: info.key,
            }),
        },
    }
}
