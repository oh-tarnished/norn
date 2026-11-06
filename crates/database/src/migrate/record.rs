//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `<database>/MIGRATION`: how far a migration step has got, so a restart continues it.
//!
//! Text, with a CRC-32C last line, replaced atomically at each change of phase:
//!
//! ```text
//! norn-migration 1
//! from 1
//! to 2
//! phase swap
//! damage 0
//! file streams/0000000000000001/0000000000000000-g0000.nseg
//! crc 12345678
//! ```

use std::fmt::Write as _;
use std::io::ErrorKind;
use std::path::Path;

use norn_storage::{atomic_write, crc32c};

use crate::error::{DatabaseError, Result};
use crate::version::MIGRATION_FILE;

/// A step's progress. The order is the crash guarantee.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Phase {
    /// New files are being written beside the old. Repeating starts them over.
    Prepare,
    /// The new files are durable, and the list below is final: the commit point.
    Swap,
    /// Every file is in place and the store version moved. What is left: check the
    /// result, then remove the old files.
    Finish,
}

/// One step's record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub(super) from: u16,
    pub(super) to: u16,
    pub(super) phase: Phase,
    /// Damaged regions found before this step began: a step must not add to them.
    pub(super) damage: u64,
    /// The files the step rewrites. Empty before [`Phase::Swap`].
    pub(super) files: Vec<String>,
}

impl Record {
    pub(super) fn write(&self, root: &Path) -> Result<()> {
        let phase = match self.phase {
            Phase::Prepare => "prepare",
            Phase::Swap => "swap",
            Phase::Finish => "finish",
        };
        let mut s = format!(
            "norn-migration 1\nfrom {}\nto {}\nphase {phase}\ndamage {}\n",
            self.from, self.to, self.damage
        );
        for file in &self.files {
            let _ = writeln!(s, "file {file}");
        }
        let crc = crc32c(s.as_bytes());
        let _ = writeln!(s, "crc {crc:08x}");
        Ok(atomic_write(&root.join(MIGRATION_FILE), s.as_bytes())?)
    }

    /// The record in `root`, if a step is under way. Copies.
    pub(super) fn read(root: &Path) -> Result<Option<Self>> {
        let path = root.join(MIGRATION_FILE);
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(DatabaseError::io("read", path, e)),
        };
        parse(&text)
            .map(Some)
            .ok_or_else(|| DatabaseError::corrupt(path, "MIGRATION fails its checksum"))
    }
}

fn parse(text: &str) -> Option<Record> {
    let cut = text.trim_end_matches('\n').rfind('\n')? + 1;
    let (body, last) = text.split_at(cut);
    let crc = last.trim_end().strip_prefix("crc ")?;
    (u32::from_str_radix(crc, 16).ok()? == crc32c(body.as_bytes())).then_some(())?;
    let mut lines = body.lines();
    (lines.next()? == "norn-migration 1").then_some(())?;
    let mut record = Record {
        from: lines.next()?.strip_prefix("from ")?.parse().ok()?,
        to: lines.next()?.strip_prefix("to ")?.parse().ok()?,
        phase: match lines.next()?.strip_prefix("phase ")? {
            "prepare" => Phase::Prepare,
            "swap" => Phase::Swap,
            "finish" => Phase::Finish,
            _ => return None,
        },
        damage: lines.next()?.strip_prefix("damage ")?.parse().ok()?,
        files: Vec::new(),
    };
    for line in lines {
        let file = line.strip_prefix("file ")?;
        // The record names files in this database and nothing else.
        (!file.split('/').any(|p| p == ".." || p.is_empty())).then_some(())?;
        record.files.push(file.to_owned());
    }
    Some(record)
}
