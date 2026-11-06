//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a `MANIFEST`, believing nothing until its checksum passes.

use std::path::Path;

use norn_storage::crc32c;

use super::manifest::{Entry, MANIFEST_VERSION, Manifest, Origin, is_database_path};
use crate::error::{DatabaseError, Result};

impl Manifest {
    /// Parses a manifest read from `path`, which is only named in errors.
    ///
    /// # Errors
    /// [`DatabaseError::Corrupt`] for a failed CRC or a line that does not parse;
    /// a layout newer than this build's is also that, naming the version.
    pub fn decode(text: &str, path: &Path) -> Result<Self> {
        let bad = |what: String| DatabaseError::corrupt(path, what);
        let body_len = text
            .trim_end_matches('\n')
            .rfind('\n')
            .map_or(0, |at| at + 1);
        let (body, last) = text.split_at(body_len);
        let crc = last.trim_end().strip_prefix("crc ").and_then(hex);
        if crc != Some(crc32c(body.as_bytes())) {
            return Err(bad(
                "MANIFEST fails its CRC-32C: it is cut short or damaged".into(),
            ));
        }
        let mut lines = body.lines();
        let version = lines.next().and_then(|l| l.strip_prefix("norn-backup "));
        match version.and_then(|v| v.parse::<u16>().ok()) {
            Some(MANIFEST_VERSION) => {}
            Some(found) => {
                return Err(bad(format!(
                    "MANIFEST layout {found} is newer than this Norn's {MANIFEST_VERSION}"
                )));
            }
            None => return Err(bad("this is not a Norn backup manifest".into())),
        }
        let mut out = Self {
            id: String::new(),
            created_unix_nanos: 0,
            store_version: 0,
            norn_version: String::new(),
            parent: None,
            files: Vec::new(),
        };
        for line in lines {
            let parsed = out.take(line);
            parsed.ok_or_else(|| bad(format!("MANIFEST cannot be read at `{line}`")))?;
        }
        Ok(out)
    }

    fn take(&mut self, line: &str) -> Option<()> {
        let (key, value) = line.split_once(' ')?;
        match key {
            "id" => value.clone_into(&mut self.id),
            "created_unix_nanos" => self.created_unix_nanos = value.parse().ok()?,
            "store_version" => self.store_version = value.parse().ok()?,
            "norn_version" => value.clone_into(&mut self.norn_version),
            "parent" => self.parent = Some(value.to_owned()),
            "file" => self.files.push(entry(value)?),
            _ => return None,
        }
        Some(())
    }
}

fn hex(text: &str) -> Option<u32> {
    (text.len() == 8).then(|| u32::from_str_radix(text, 16).ok())?
}

fn entry(value: &str) -> Option<Entry> {
    let words: Vec<_> = value.split(' ').collect();
    let [path, len, crc, tail, origin] = words.as_slice() else {
        return None;
    };
    let origin = match *origin {
        "here" => Origin::Here,
        "parent" => Origin::Parent,
        _ => return None,
    };
    is_database_path(path).then_some(())?;
    Some(Entry {
        path: (*path).to_owned(),
        len: len.parse().ok()?,
        crc: hex(crc)?,
        tail: hex(tail)?,
        origin,
    })
}
