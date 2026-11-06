//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The metadata log's replay, exposed for the fuzz crate only (feature
//! `fuzzing`). [`replay`] is the scan [`crate::Registry::open`] runs over the
//! bytes of `metadata.log`, applying every record to the same in-memory state,
//! without the lock file, the fsyncs or the torn-tail side file around it. Not
//! an API: no shipped build enables the feature.

use std::path::Path;

use crate::error::MetadataError;
use crate::log::replay::{header, scan};
use crate::op::Op;
use crate::registry::State;

/// The header a current log starts with, so a target can build one whose
/// records it controls.
#[must_use]
pub fn log_header() -> Vec<u8> {
    header()
}

/// What replaying a log found.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Replayed {
    /// Where the good records end; the rest is a torn tail.
    pub end: usize,
    /// Records replayed.
    pub records: u64,
    /// Entities, streams and signals alive afterwards.
    pub live: u64,
}

/// Replays a whole log's `bytes`, as opening the registry does. Borrows.
///
/// # Errors
/// [`MetadataError::Corrupt`] or [`MetadataError::UnsupportedVersion`], as
/// opening would return.
pub fn replay(bytes: &[u8]) -> Result<Replayed, MetadataError> {
    let mut state = State::default();
    let (end, records, _) = scan(bytes, Path::new("metadata.log"), |op| state.apply(op))?;
    let live = state.live();
    Ok(Replayed { end, records, live })
}

/// Decodes one record's payload and encodes it again; `None` if it is not a
/// record. Decoding the result must give the same record back.
#[must_use]
pub fn reencode(payload: &[u8]) -> Option<Vec<u8>> {
    Op::decode(payload).map(|op| op.encode())
}

/// Whether `payload` decodes to exactly the record `again` decodes to.
#[must_use]
pub fn same_record(payload: &[u8], again: &[u8]) -> bool {
    Op::decode(payload).is_some_and(|a| Op::decode(again).is_some_and(|b| a == b))
}
