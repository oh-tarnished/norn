//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The write-ahead journal's readers, exposed for the fuzz crate only (feature
//! `fuzzing`): the batch record decoder with its encoder, and the replay of one
//! journal segment file exactly as startup recovery runs it. Not an API: no
//! shipped build enables the feature, and these may change without notice.

use std::path::Path;

use norn_core::Sample;
use norn_storage::{StorageError, StorageOptions};

use super::{IngestRecovery, record, recover};

/// Decodes one journal batch record, as replay does after its frame's CRC
/// passed. Copies; `None` for anything that is not one.
#[must_use]
pub fn decode_record(bytes: &[u8]) -> Option<Vec<Sample>> {
    record::decode(bytes)
}

/// Encodes `samples` as one journal batch record.
///
/// # Errors
/// A value too large for its encoding.
pub fn encode_record(samples: &[Sample]) -> Result<Vec<u8>, norn_core::Error> {
    let mut out = Vec::new();
    record::encode(samples, &mut out)?;
    Ok(out)
}

/// Replays the journal segment at `path` under `opts`, appending its samples
/// to `samples`; true when every batch was readable. Reads only.
///
/// # Errors
/// What opening the segment returns.
pub fn read_journal(
    path: &Path,
    opts: &StorageOptions,
    samples: &mut Vec<Sample>,
) -> Result<bool, StorageError> {
    let mut total = IngestRecovery::default();
    recover::read_journal(path, opts, samples, &mut total)
}
