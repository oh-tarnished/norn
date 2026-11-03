//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The Arrow IPC file format: the same columns, for tools that map Arrow directly.
//! One record batch per query response; no compression (its codecs are C-backed
//! here), so it trades size for needing no decoder.

use std::collections::HashMap;
use std::io::{Read, Seek, Write};

use arrow_ipc::reader::FileReader;
use arrow_ipc::writer::FileWriter;
use arrow_schema::ArrowError;
use norn_adapter::{
    AdapterError, CATALOG_KEY, Catalog, DEFAULT_MAX_SIGNALS, Discovery, SampleSink, SampleSource,
    SourceBatch,
};
use norn_protocol::common::Sample;
use norn_protocol::query::SampleOrder;

use crate::DIGESTS_KEY;
use crate::columns::{schema, to_batch};
use crate::digest::{self, Digest, Part, encode};
use crate::from_batch::from_batch;

const FORMAT: &str = "arrow";
/// Rows in a record batch; a reader holds one at a time.
const BATCH_ROWS: usize = 16 * 1024;

fn failed(e: ArrowError) -> AdapterError {
    AdapterError::Encode {
        format: FORMAT,
        detail: e.to_string(),
    }
}

/// A [`SampleSink`] writing the Arrow IPC file format. Holds a record batch and a
/// digest per batch. Concurrency: one task at a time.
pub struct ArrowSink<W: Write> {
    writer: FileWriter<W>,
    streams: HashMap<String, String>,
    digest: Digest,
    parts: Vec<Part>,
}

impl<W: Write + Seek> ArrowSink<W> {
    /// A sink writing to `out`.
    ///
    /// # Errors
    /// [`AdapterError::Encode`] when the file's start cannot be written.
    pub fn new(out: W) -> Result<Self, AdapterError> {
        Ok(Self {
            writer: FileWriter::try_new(out, &schema()).map_err(failed)?,
            streams: HashMap::new(),
            digest: Digest::default(),
            parts: Vec::new(),
        })
    }
}

impl<W: Write + Seek> SampleSink for ArrowSink<W> {
    fn order(&self) -> SampleOrder {
        SampleOrder::BySignal
    }

    fn begin(&mut self, catalog: &Catalog) -> Result<(), AdapterError> {
        for signal in &catalog.signals {
            (self.streams).insert(signal.name.clone(), signal.stream.clone());
        }
        self.writer.write_metadata(CATALOG_KEY, catalog.to_json()?);
        Ok(())
    }

    fn write(&mut self, samples: &[Sample]) -> Result<(), AdapterError> {
        for part in samples.chunks(BATCH_ROWS) {
            let batch = to_batch(part, &self.streams, FORMAT)?;
            self.writer.write(&batch).map_err(failed)?;
            self.digest.update(part);
            self.parts.push(self.digest.seal());
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, AdapterError> {
        self.writer.write_metadata(DIGESTS_KEY, encode(&self.parts));
        self.writer.finish().map_err(failed)?;
        self.writer.flush().map_err(failed)?;
        let bytes = self.writer.get_mut().stream_position();
        bytes.map_err(AdapterError::io("sizing the Arrow file"))
    }
}

/// A [`SampleSource`] reading the Arrow IPC file format, one batch at a time.
///
/// Holds one record batch, the footer, and a line per signal. Concurrency: one
/// task at a time.
pub struct ArrowSource<R: Read + Seek> {
    input: Option<R>,
    reader: Option<FileReader<R>>,
    parts: Vec<Part>,
    done: usize,
    rows: u64,
    discovery: Discovery,
}

impl<R: Read + Seek> ArrowSource<R> {
    /// A source reading `input`. Reads nothing until [`SampleSource::catalog`].
    #[must_use]
    pub fn new(input: R) -> Self {
        Self {
            input: Some(input),
            reader: None,
            parts: Vec::new(),
            done: 0,
            rows: 0,
            discovery: Discovery::new(FORMAT, DEFAULT_MAX_SIGNALS),
        }
    }
}

impl<R: Read + Seek> SampleSource for ArrowSource<R> {
    fn catalog(&mut self) -> Result<Catalog, AdapterError> {
        let Some(input) = self.input.take() else {
            return Ok(Catalog::default());
        };
        let reader = FileReader::try_new(input, None).map_err(|e| AdapterError::Truncated {
            format: FORMAT,
            row: None,
            detail: format!("no readable footer: {e}"),
        })?;
        let found = |key: &str| reader.custom_metadata().get(key).cloned();
        self.parts = digest::decode(&found(DIGESTS_KEY).unwrap_or_default(), FORMAT)?;
        let catalog = match found(CATALOG_KEY) {
            Some(json) => Catalog::from_json(&json, FORMAT)?,
            None => Catalog::default(),
        };
        catalog
            .signals
            .iter()
            .for_each(|s| self.discovery.declare(s));
        self.reader = Some(reader);
        Ok(catalog)
    }

    fn next_batch(&mut self) -> Result<Option<SourceBatch>, AdapterError> {
        let row = self.rows + 1;
        let corrupt = |detail: String| AdapterError::Corrupt {
            format: FORMAT,
            row: Some(row),
            detail,
        };
        let Some(next) = self.reader.as_mut().and_then(Iterator::next) else {
            return Ok(None);
        };
        let batch = next.map_err(|e| corrupt(e.to_string()))?;
        let out = from_batch(&batch, row, FORMAT, &mut self.discovery)?;
        let mut digest = Digest::default();
        digest.update(&out.samples);
        let got = digest.seal();
        if self.parts.get(self.done).is_some_and(|want| *want != got) {
            return Err(corrupt(format!("the batch's checksum is {:08x}", got.crc)));
        }
        self.done += 1;
        self.rows += out.samples.len() as u64;
        Ok(Some(out))
    }
}
