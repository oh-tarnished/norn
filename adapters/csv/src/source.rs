//! Reading the format, in bounded memory.

use std::io::BufRead;

use norn_adapter::{
    AdapterError, Catalog, DEFAULT_MAX_SIGNALS, Discovery, SampleSource, SourceBatch,
};

use crate::lines::Lines;
use crate::row::{malformed, parse};
use crate::{FORMAT, HEADER, PREAMBLE, TRAILER};

/// Rows a batch holds.
const BATCH_ROWS: usize = 4_096;
/// Bytes of rows a batch holds, whichever comes first.
const BATCH_BYTES: usize = 4 * 1024 * 1024;
/// The longest record: above a 4 MiB value in hex and quotes.
const DEFAULT_MAX_RECORD_BYTES: u64 = 16 * 1024 * 1024;
/// The longest first line: the whole catalog.
const MAX_CATALOG_BYTES: u64 = 1024 * 1024 * 1024;

/// A [`SampleSource`] over any buffered reader.
///
/// Holds one batch, one record of at most the record limit, and a line per
/// signal. Rows count from 1, without the header. Reads forward only.
/// Concurrency: used by one task at a time.
#[derive(Debug)]
pub struct CsvSource<R> {
    lines: Lines<R>,
    rows: u64,
    promised: bool,
    finished: bool,
    max_record_bytes: u64,
    digest: crc32fast::Hasher,
    discovery: Discovery,
}

impl<R: BufRead> CsvSource<R> {
    /// A source reading `input`. Reads nothing until [`SampleSource::catalog`].
    #[must_use]
    pub fn new(input: R) -> Self {
        Self {
            lines: Lines::new(input),
            rows: 0,
            promised: false,
            finished: false,
            max_record_bytes: DEFAULT_MAX_RECORD_BYTES,
            digest: crc32fast::Hasher::new(),
            discovery: Discovery::new(FORMAT, DEFAULT_MAX_SIGNALS),
        }
    }

    /// Refuses a record longer than this, rather than buffer it.
    #[must_use]
    pub fn max_record_bytes(mut self, bytes: u64) -> Self {
        self.max_record_bytes = bytes;
        self
    }

    fn truncated(&self, detail: &str) -> AdapterError {
        AdapterError::Truncated {
            format: FORMAT,
            row: Some(self.rows + 1),
            detail: detail.to_owned(),
        }
    }

    /// Checks the promise of the last line, and that nothing follows it.
    fn check_trailer(&mut self) -> Result<(), AdapterError> {
        let row = self.rows;
        let text = std::str::from_utf8(&self.lines.line).unwrap_or_default();
        let body = text.strip_prefix(TRAILER).unwrap_or_default().trim_end();
        let crc = self.digest.clone().finalize();
        if !self.promised {
            return Err(malformed(Some(row + 1), "an end line with no first line"));
        }
        let want = format!("rows={row} crc32={crc:08x}");
        if body != want {
            return Err(AdapterError::Corrupt {
                format: FORMAT,
                row: None,
                detail: format!("the file says {body:?}, its {row} rows give {want:?}"),
            });
        }
        if self.lines.read_record(self.max_record_bytes)? {
            return Err(malformed(Some(row + 1), "text after the end line"));
        }
        self.finished = true;
        Ok(())
    }
}

impl<R: BufRead> SampleSource for CsvSource<R> {
    fn catalog(&mut self) -> Result<Catalog, AdapterError> {
        if self.lines.read_line(MAX_CATALOG_BYTES)? == 0 {
            return Err(self.truncated("the file is empty"));
        }
        let mut catalog = Catalog::default();
        let first = std::str::from_utf8(&self.lines.line).unwrap_or_default();
        if let Some(json) = first.strip_prefix(PREAMBLE) {
            if !first.ends_with('\n') {
                return Err(self.truncated("the first line never ends"));
            }
            catalog = Catalog::from_json(json, FORMAT)?;
            let declared = catalog.signals.iter();
            declared.for_each(|s| self.discovery.declare(s));
            self.promised = true;
            self.lines.read_line(self.max_record_bytes)?;
        }
        let header = std::str::from_utf8(&self.lines.line).unwrap_or_default();
        let header = header.trim_end();
        if header != HEADER {
            let detail = format!("the header is {header:?}, not {HEADER:?}");
            return Err(malformed(None, detail));
        }
        Ok(catalog)
    }

    fn next_batch(&mut self) -> Result<Option<SourceBatch>, AdapterError> {
        let mut batch = SourceBatch::default();
        let mut bytes = 0;
        while !self.finished && batch.samples.len() < BATCH_ROWS && bytes < BATCH_BYTES {
            if !self.lines.read_record(self.max_record_bytes)? {
                if self.promised {
                    return Err(self.truncated("no end line: the file was cut at a row"));
                }
                self.finished = true;
                break;
            }
            if self.lines.line.starts_with(TRAILER.as_bytes()) {
                self.check_trailer()?;
                break;
            }
            let blank = self.lines.line.iter().all(u8::is_ascii_whitespace);
            if blank && !self.promised {
                continue;
            }
            if self.promised && self.lines.line.last() != Some(&b'\n') {
                return Err(self.truncated("the row is cut off"));
            }
            let row = self.rows + 1;
            parse(&self.lines.line, row, &mut self.discovery, &mut batch)?;
            self.rows = row;
            self.digest.update(&self.lines.line);
            bytes += self.lines.line.len();
        }
        let empty = batch.samples.is_empty();
        Ok((!empty).then_some(batch))
    }
}
