//! Pulling bounded lines and records out of a reader.

use std::io::{BufRead, Read};

use norn_adapter::AdapterError;

/// A reader of `\n`-ended lines, keeping the last in [`line`](Self::line).
#[derive(Debug)]
pub(crate) struct Lines<R> {
    input: R,
    pub(crate) line: Vec<u8>,
}

impl<R: BufRead> Lines<R> {
    pub(crate) fn new(input: R) -> Self {
        Self {
            input,
            line: Vec::new(),
        }
    }

    /// Reads to the next `\n`, onto `line`, up to `limit` bytes in all.
    fn extend(&mut self, limit: u64) -> Result<usize, AdapterError> {
        let held = self.line.len() as u64;
        let left = limit.saturating_sub(held).saturating_add(1);
        let mut part = (&mut self.input).take(left);
        let n = part.read_until(b'\n', &mut self.line);
        let n = n.map_err(AdapterError::io("reading the CSV file"))?;
        if self.line.len() as u64 > limit {
            return Err(AdapterError::LimitExceeded {
                limit: "max_record_bytes",
                value: limit,
            });
        }
        Ok(n)
    }

    /// Replaces `line` with the next line. Zero bytes means the end of the file.
    pub(crate) fn read_line(&mut self, limit: u64) -> Result<usize, AdapterError> {
        self.line.clear();
        self.extend(limit)
    }

    /// Replaces `line` with the next record, across quoted newlines. False at the
    /// end of the file. The last record may have no `\n`.
    pub(crate) fn read_record(&mut self, limit: u64) -> Result<bool, AdapterError> {
        self.line.clear();
        let (mut quotes, mut seen) = (0, 0);
        loop {
            let n = self.extend(limit)?;
            let part = self.line.get(seen..).unwrap_or_default();
            quotes += part.iter().filter(|b| **b == b'"').count();
            seen = self.line.len();
            if n == 0 || (self.line.last() == Some(&b'\n') && quotes % 2 == 0) {
                return Ok(!self.line.is_empty());
            }
        }
    }
}
