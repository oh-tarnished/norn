//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The file-system side of a Unix socket: directory hardening, stale-socket
//! detection and owner-checked removal (SPEC §41).

use std::fs::{self, DirBuilder, Permissions};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::net::UnixStream;

use crate::TransportError;

/// How long a probe of an existing socket may take before the socket is
/// presumed live. A live server answers in microseconds; a full backlog
/// could otherwise stall the probe.
const PROBE_TIMEOUT: Duration = Duration::from_secs(1);

/// Ensures `dir` exists, is a directory and is owner-only.
///
/// A missing directory is created (with any missing parents) as 0700. An
/// existing one is refused if it is a symbolic link, which another user can
/// plant in `/tmp`, or if group or others hold any permission bit. Its owner is
/// checked once the socket exists ([`check_owner`]): root can create a socket in
/// a 0700 directory another user owns.
pub(super) fn prepare_directory(dir: &Path) -> Result<(), TransportError> {
    if !dir.exists() {
        DirBuilder::new().recursive(true).mode(0o700).create(dir)?;
    }
    let meta = fs::symlink_metadata(dir)?;
    if meta.file_type().is_symlink() {
        let msg = format!("{} is a symbolic link, not a directory", dir.display());
        return Err(io::Error::new(io::ErrorKind::NotADirectory, msg).into());
    }
    if !meta.is_dir() {
        let msg = format!("{} is not a directory", dir.display());
        return Err(io::Error::new(io::ErrorKind::NotADirectory, msg).into());
    }
    let mode = meta.mode() & 0o7777;
    if mode & 0o077 != 0 {
        return Err(TransportError::InsecureDirectory {
            path: dir.to_path_buf(),
            mode,
        });
    }
    Ok(())
}

/// Clears `path` for binding: nothing there is fine; a socket nobody
/// answers on is stale and removed; a live socket or any other kind of
/// file is refused and left untouched.
pub(super) async fn clear_stale(path: &Path) -> Result<(), TransportError> {
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) => return Err(e.into()),
    };
    if !meta.file_type().is_socket() {
        return Err(TransportError::NotASocket(path.to_path_buf()));
    }
    match tokio::time::timeout(PROBE_TIMEOUT, UnixStream::connect(path)).await {
        Ok(Err(e)) if e.kind() == io::ErrorKind::ConnectionRefused => {
            fs::remove_file(path)?;
            Ok(())
        }
        Ok(Err(e)) => Err(e.into()),
        Ok(Ok(_)) | Err(_) => Err(TransportError::LiveSocket(path.to_path_buf())),
    }
}

/// Restricts the freshly bound socket to its owner (0600).
///
/// Belt and braces: the directory is already 0700, but the socket must not
/// rely on the umask it happened to be created under.
pub(super) fn restrict_socket(path: &Path) -> io::Result<()> {
    fs::set_permissions(path, Permissions::from_mode(0o600))
}

/// Refuses a socket directory owned by anyone but the user the socket was just
/// created as. Compared with the socket's owner because std has no `geteuid`, and
/// this crate no `unsafe` on Unix.
pub(super) fn check_owner(dir: &Path, socket: &Path) -> io::Result<()> {
    let (owner, us) = (
        fs::metadata(dir)?.uid(),
        fs::symlink_metadata(socket)?.uid(),
    );
    if owner == us {
        return Ok(());
    }
    let msg = format!(
        "{} belongs to user {owner}, not to {us}: another user could reach the socket",
        dir.display()
    );
    Err(io::Error::new(io::ErrorKind::PermissionDenied, msg))
}

/// Removes the socket file on drop, but only while it is still the file
/// this process bound: identified by device and inode, so a socket another
/// server has since put at the same path is never deleted.
///
/// Not `Clone`: exactly one guard owns a socket file.
#[derive(Debug)]
pub(super) struct SocketGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
}

impl SocketGuard {
    /// Records the identity of the socket now at `path`.
    pub(super) fn new(path: &Path) -> io::Result<Self> {
        let meta = fs::symlink_metadata(path)?;
        Ok(Self {
            path: path.to_path_buf(),
            dev: meta.dev(),
            ino: meta.ino(),
        })
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        let ours = fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.file_type().is_socket() && m.dev() == self.dev && m.ino() == self.ino);
        if ours {
            // Nothing useful can be done with a failure inside drop; the
            // next bind treats a leftover socket as stale and replaces it.
            let _ = fs::remove_file(&self.path);
        }
    }
}
