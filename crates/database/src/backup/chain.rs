//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An incremental backup and the backups it builds on, and checking them all.

use std::path::{Path, PathBuf};

use super::copy::measure;
use super::manifest::{Entry, Manifest, Origin};
use crate::error::{DatabaseError, Result};
use crate::version::CURRENT_VERSION;

/// How long a chain may be. A longer one is much more likely a loop in a damaged
/// manifest; take a full backup rather than chain this far.
const MAX_CHAIN: usize = 256;

/// A backup's manifest and those of its ancestors, nearest first.
///
/// Memory: every manifest in the chain.
#[derive(Debug)]
pub(super) struct Chain {
    links: Vec<(PathBuf, Manifest)>,
}

impl Chain {
    /// Reads `from` and all its ancestors, checking each manifest.
    pub(super) fn load(from: &Path) -> Result<Self> {
        let mut links = Vec::new();
        let mut at = from.to_path_buf();
        loop {
            let manifest = Manifest::read(&at)?;
            if manifest.store_version > CURRENT_VERSION {
                return Err(DatabaseError::TooNew {
                    found: manifest.store_version,
                    supported: CURRENT_VERSION,
                });
            }
            let next = manifest.parent.as_ref().map(|p| at.join(p));
            links.push((at, manifest));
            let Some(next) = next else {
                return Ok(Self { links });
            };
            if links.len() >= MAX_CHAIN {
                let what = format!("its chain of backups is longer than {MAX_CHAIN}");
                return Err(DatabaseError::corrupt(from, what));
            }
            at = next;
        }
    }

    /// The newest manifest: the state the chain restores.
    pub(super) fn head(&self) -> Option<&Manifest> {
        self.links.first().map(|(_, m)| m)
    }

    /// Where `entry`'s bytes are, following its origin up the chain. An ancestor that
    /// disagrees about the file is damage, as is one that does not list it.
    pub(super) fn source(&self, entry: &Entry) -> Result<PathBuf> {
        for (dir, manifest) in &self.links {
            let found = manifest.files.iter().find(|f| f.path == entry.path);
            let Some(found) = found else { break };
            if (found.len, found.crc) != (entry.len, entry.crc) {
                break;
            }
            if found.origin == Origin::Here {
                return Ok(dir.join(&entry.path));
            }
        }
        let what = format!("no backup in its chain holds {}", entry.path);
        Err(DatabaseError::corrupt(
            self.links.first().map_or(Path::new(""), |(d, _)| d),
            what,
        ))
    }
}

/// What checking a backup found: it is whole.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct BackupCheck {
    /// Files checked.
    pub files: u64,
    /// Bytes read.
    pub bytes: u64,
    /// Backups in the chain, this one included.
    pub chain: u64,
}

/// Reads every file of the backup at `from`, wherever in its chain it lives, against
/// the checksums in the manifest. Changes nothing, and needs no key.
///
/// # Errors
/// [`DatabaseError::Mismatch`] for a damaged file; [`DatabaseError::Io`] for a missing
/// one, or a backup with no manifest; as [`Manifest::read`].
pub fn check_backup(from: &Path) -> Result<BackupCheck> {
    let chain = Chain::load(from)?;
    let mut check = BackupCheck {
        chain: chain.links.len() as u64,
        ..BackupCheck::default()
    };
    for entry in chain.head().map_or(&[][..], |m| &m.files) {
        let path = chain.source(entry)?;
        let found = measure(&path)?;
        if found != (entry.len, entry.crc) {
            let expected = (entry.len, entry.crc);
            return Err(DatabaseError::Mismatch {
                path,
                expected,
                found,
            });
        }
        check.files += 1;
        check.bytes += entry.len;
    }
    Ok(check)
}
