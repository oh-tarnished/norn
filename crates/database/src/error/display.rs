//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How each error reads: one line, naming the command that fixes it.

use std::fmt;

use super::{DatabaseError, Unfinished};

impl fmt::Display for DatabaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { op, path, source } => write!(f, "{op} {}: {source}", path.display()),
            Self::Storage(e) => write!(f, "{e}"),
            Self::Metadata(e) => write!(f, "{e}"),
            Self::Locked { path } => write!(
                f,
                "the database is in use ({} is held): stop the daemon first",
                path.display()
            ),
            Self::Corrupt { path, what } => write!(f, "{}: {what}", path.display()),
            Self::TooNew { found, supported } => write!(
                f,
                "store version {found} was written by a newer Norn; this one reads up to \
                 {supported}, and nothing was changed"
            ),
            Self::NeedsMigration {
                root,
                found,
                current,
            } => write!(
                f,
                "store version {found} must be migrated before this Norn opens it: run \
                 nornctl migrate --data-dir {} --to {current}",
                root.display()
            ),
            Self::TooOld { found, oldest } => write!(
                f,
                "store version {found} is older than the oldest this Norn migrates ({oldest})"
            ),
            Self::Unfinished { root, job } => {
                let cmd = match job {
                    Unfinished::Migration => "nornctl migrate",
                    Unfinished::Restore => "nornctl restore",
                };
                write!(
                    f,
                    "{} holds an unfinished {cmd}: run it again",
                    root.display()
                )
            }
            Self::NoMigration { from, to } => {
                write!(f, "no migration leads from store version {from} to {to}")
            }
            Self::NotEmpty { path } => write!(f, "{} is not empty", path.display()),
            Self::Mismatch {
                path,
                expected,
                found,
            } => write!(
                f,
                "{}: expected {} bytes with CRC-32C {:08x}, found {} with {:08x}",
                path.display(),
                expected.0,
                expected.1,
                found.0,
                found.1
            ),
            Self::Damaged { root, regions } => {
                write!(f, "{}: {regions} damaged regions", root.display())
            }
            Self::Interrupted => f.write_str("interrupted at a test's stop point"),
        }
    }
}

impl std::error::Error for DatabaseError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Storage(e) => Some(e),
            Self::Metadata(e) => Some(e),
            _ => None,
        }
    }
}
