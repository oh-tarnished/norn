//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a segment header.

use std::path::Path;

use norn_codec::CODEC_VERSION;
use norn_core::StreamId;

use super::header::FLAG_ENCRYPTED;
use super::{
    FORMAT_MAJOR, FORMAT_MINOR, HEADER_LEN, Le, SCHEMA_VERSION, SEGMENT_MAGIC, SegmentHeader,
    sealing,
};
use crate::crc32c::crc32c;
use crate::error::{CorruptionKind, Result, StorageError, VersionField};

impl SegmentHeader {
    /// Parses and checks a header read from `path` (named in errors only),
    /// and the encryption record after it when its flag is set. Copies;
    /// borrows nothing past the call.
    ///
    /// # Errors
    /// [`StorageError::Corrupt`] with [`CorruptionKind::TornTail`] for fewer
    /// than [`HEADER_LEN`] bytes, `BadMagic` or `HeaderChecksum`; and
    /// [`StorageError::UnsupportedVersion`] for any version newer than this
    /// build's (checked only once the CRC passes, so a bit flip is reported
    /// as corruption, not as a newer file).
    pub fn decode(bytes: &[u8], path: &Path) -> Result<Self> {
        let bad = |kind| StorageError::corrupt(path, 0, HEADER_LEN as u64, kind);
        let rec = bytes
            .get(..HEADER_LEN)
            .ok_or(bad(CorruptionKind::TornTail))?;
        let (body, crc) = rec.split_at(HEADER_LEN - 4);
        let mut r = Le::new(rec);
        if r.bytes::<8>() != Some(SEGMENT_MAGIC) {
            return Err(bad(CorruptionKind::BadMagic));
        }
        if crc != crc32c(body).to_le_bytes() {
            return Err(bad(CorruptionKind::HeaderChecksum));
        }
        let malformed = || bad(CorruptionKind::Malformed("segment header"));
        let format = r.u16().ok_or_else(malformed)?;
        let schema = r.u16().ok_or_else(malformed)?;
        let codec = r.u8().ok_or_else(malformed)?;
        let flags = r.u8().ok_or_else(malformed)?;
        let generation = r.u16().ok_or_else(malformed)?;
        let stream = r.u64().ok_or_else(malformed)?;
        let first_seq = r.u64().ok_or_else(malformed)?;
        let last_seq = r.u64().ok_or_else(malformed)?;
        let [major, minor] = format.to_le_bytes();
        let newest = u16::from_le_bytes([FORMAT_MAJOR, FORMAT_MINOR]);
        if major != FORMAT_MAJOR || minor > FORMAT_MINOR {
            return Err(StorageError::UnsupportedVersion {
                path: path.to_path_buf(),
                field: VersionField::Format,
                found: format,
                supported: newest,
            });
        }
        let checks = [
            (VersionField::Schema, schema, SCHEMA_VERSION),
            (
                VersionField::Codec,
                u16::from(codec),
                u16::from(CODEC_VERSION),
            ),
        ];
        for (field, found, supported) in checks {
            if found > supported || found == 0 {
                let path = path.to_path_buf();
                return Err(StorageError::UnsupportedVersion {
                    path,
                    field,
                    found,
                    supported,
                });
            }
        }
        // 1.0 wrote the flags as zero and ignored them; 1.1 defines bit 0 only.
        let encrypted = minor >= 1 && flags & FLAG_ENCRYPTED != 0;
        if minor >= 1 && flags & !FLAG_ENCRYPTED != 0 {
            return Err(bad(CorruptionKind::Malformed("unknown segment flag")));
        }
        let encryption = match encrypted {
            true => Some(sealing::decode(
                bytes.get(HEADER_LEN..).unwrap_or_default(),
                path,
            )?),
            false => None,
        };
        Ok(Self {
            stream: StreamId::new(stream),
            first_seq,
            last_seq,
            generation,
            codec_version: codec,
            encryption,
        })
    }
}
