//! A CRC-32 over each part's samples, as their canonical protobuf bytes, so the
//! check sees every field whatever encoding the file chose.

use norn_adapter::AdapterError;
use norn_protocol::common::Sample;
use prost::Message;

/// Hashes samples into the current part. One sample's bytes are held at a time.
#[derive(Debug, Default)]
pub(crate) struct Digest {
    hasher: crc32fast::Hasher,
    rows: u64,
    scratch: Vec<u8>,
}

impl Digest {
    pub(crate) fn update(&mut self, samples: &[Sample]) {
        for sample in samples {
            self.scratch.clear();
            sample.encode_raw(&mut self.scratch);
            self.hasher.update(&self.scratch);
        }
        self.rows += samples.len() as u64;
    }

    /// `rows:crc32` of the part so far, and starts the next.
    pub(crate) fn seal(&mut self) -> Part {
        let hasher = std::mem::take(&mut self.hasher);
        let rows = std::mem::take(&mut self.rows);
        Part {
            rows,
            crc: hasher.finalize(),
        }
    }
}

/// One group's expected rows and checksum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Part {
    pub(crate) rows: u64,
    pub(crate) crc: u32,
}

/// The file form of `parts`.
pub(crate) fn encode(parts: &[Part]) -> String {
    let each = parts.iter().map(|p| format!("{}:{:08x}", p.rows, p.crc));
    each.collect::<Vec<_>>().join(",")
}

/// The parts of `text`. Copies; `format` names the file kind in errors.
pub(crate) fn decode(text: &str, format: &'static str) -> Result<Vec<Part>, AdapterError> {
    if text.is_empty() {
        return Ok(Vec::new());
    }
    let part = |item: &str| {
        let (rows, crc) = item.split_once(':')?;
        Some(Part {
            rows: rows.parse().ok()?,
            crc: u32::from_str_radix(crc, 16).ok()?,
        })
    };
    let parts: Option<Vec<Part>> = text.split(',').map(part).collect();
    parts.ok_or_else(|| AdapterError::Corrupt {
        format,
        row: None,
        detail: "the digests do not parse".to_owned(),
    })
}
