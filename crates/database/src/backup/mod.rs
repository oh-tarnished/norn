//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A backup directory: docs/guides/operations.md, "Backup and restore".
//!
//! ```text
//! <backup>/MANIFEST                      written last
//!          metadata/metadata.log         the catalogue at a commit point
//!          streams/<id>/<segment>.nseg   sealed segments, byte for byte
//! ```

mod chain;
mod copy;
mod manifest;
mod manifest_read;
mod restore;
mod summary;
mod writer;

pub use chain::{BackupCheck, check_backup};
pub use copy::{COPY_BUFFER_BYTES, is_vacant};
pub use manifest::{Entry, MANIFEST_FILE, MANIFEST_VERSION, Manifest, Origin, is_database_path};
pub use restore::{RestoreReport, restore, restore_until};
pub use summary::BackupSummary;
pub use writer::BackupWriter;
