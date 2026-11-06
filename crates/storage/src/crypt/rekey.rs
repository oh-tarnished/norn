//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Rewriting sealed segments under the current key.

use std::fs::{self, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use super::store::segment_dirs;
use super::{EncryptionError, SegmentCipher, cipher_for};
use crate::error::{Result, StorageError};
use crate::format::TRAILER_LEN;
use crate::format::directory::decode_trailer;
use crate::fsio::{Source, tmp_path};
use crate::layout::list_segments_lenient;
use crate::lock::StreamLock;
use crate::options::StorageOptions;
use crate::platform::sync_dir;
use crate::segment::load::{load_directory, read_frame, read_header};

/// What [`rekey_database`] did.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct RekeyReport {
    /// Segments rewritten under the current key.
    pub rewritten: u64,
    /// Segments already under it.
    pub current: u64,
    /// Segments still under an older key, with why: damaged or unsealed. While
    /// there is any, the older key must stay in the key file.
    pub left: Vec<(PathBuf, String)>,
}

/// Rewrites every segment under `root` that is under an older key, so the older
/// keys can be removed from the key file. Each file is rewritten beside itself and
/// renamed over the original; offsets and lengths do not change, so the index and
/// clean markers stay valid. A crash leaves the original and a `.nseg.tmp`, which
/// recovery removes. A segment that cannot be read whole is left as it is.
///
/// Run with no daemon and after recovery, which seals what a crash left unsealed.
///
/// # Errors
/// [`StorageError::Locked`] when a writer holds a stream;
/// [`EncryptionError::Mismatch`] for a plain store; [`StorageError::Io`].
pub fn rekey_database(root: &Path, opts: &StorageOptions) -> Result<RekeyReport> {
    let Some(keys) = opts.keyring else {
        let path = root.to_path_buf();
        let encrypted = false;
        return Err(StorageError::Encryption(EncryptionError::Mismatch {
            path,
            encrypted,
        }));
    };
    let mut report = RekeyReport::default();
    for dir in segment_dirs(root)? {
        let _lock = StreamLock::acquire(&dir)?;
        for entry in list_segments_lenient(&dir)? {
            let older = entry
                .header
                .encryption
                .is_none_or(|info| info.key != keys.current());
            if !older {
                report.current += 1;
                continue;
            }
            match rewrite(&entry.path, opts) {
                Ok(()) => report.rewritten += 1,
                Err(e @ (StorageError::Corrupt(_) | StorageError::Sealed)) => {
                    let _ = fs::remove_file(tmp_path(&entry.path));
                    report.left.push((entry.path, e.to_string()));
                }
                Err(e) => {
                    let _ = fs::remove_file(tmp_path(&entry.path));
                    return Err(e);
                }
            }
        }
    }
    Ok(report)
}

/// The one segment at `path`, read under its key and rewritten under the current.
fn rewrite(path: &Path, opts: &StorageOptions) -> Result<()> {
    let mut src = Source::open(path)?;
    let mut header = read_header(&mut src)?;
    let old = cipher_for(&header, opts, path)?;
    let Some(chunks) = load_directory(&mut src, opts)? else {
        return Err(StorageError::Sealed);
    };
    let new = SegmentCipher::for_new(&mut header, opts)?;
    let new = new.ok_or(StorageError::Encryption(EncryptionError::Random))?;
    let bytes = header.encode_all();
    let tmp = tmp_path(path);
    let on = |op, e| StorageError::io(op, &tmp, e);
    let _ = fs::remove_file(&tmp);
    let mut out = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .map_err(|e| on("create", e))?;
    out.write_all(&bytes).map_err(|e| on("write", e))?;
    let mut buf = Vec::new();
    for meta in &chunks {
        read_frame(&mut src, meta, opts, old.as_ref(), &mut buf)?;
        new.seal_frame(&mut buf, 0, meta, meta.offset)?;
        out.seek(SeekFrom::Start(meta.offset))
            .and_then(|_| out.write_all(&buf))
            .map_err(|e| on("write", e))?;
    }
    let mut trailer = [0u8; TRAILER_LEN];
    src.read_at(src.len() - TRAILER_LEN as u64, &mut trailer)?;
    let at = decode_trailer(&trailer).ok_or(StorageError::Sealed)?;
    let len = usize::try_from(src.len() - at).unwrap_or(usize::MAX);
    src.read_vec(at, len, &mut buf)?;
    out.seek(SeekFrom::Start(at))
        .and_then(|_| out.write_all(&buf))
        .and_then(|()| out.sync_all())
        .map_err(|e| on("write", e))?;
    drop(out);
    fs::rename(&tmp, path).map_err(|e| on("rename", e))?;
    sync_dir(crate::fsio::parent(path))
}
