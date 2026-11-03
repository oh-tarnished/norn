//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Structured logging onto stderr, with every record bounded in size.

use std::io::{self, Write};

use tracing::level_filters::LevelFilter;
use tracing_subscriber::fmt::MakeWriter;

use crate::config::{LogConfig, LogFormat, LogLevel};

const MARKER: &[u8] = b" [truncated]\n";

/// Installs the global `tracing` subscriber for `config`.
///
/// Records go synchronously to stderr: there is no queue to grow, so a slow
/// reader of stderr slows logging rather than consuming memory, and each
/// record is cut at `config.max_record_bytes` (SPEC §16, §34). Returns
/// `false`, changing nothing, if a subscriber was already installed (as in
/// a test harness).
pub fn init(config: &LogConfig) -> bool {
    let level = match config.level {
        LogLevel::Error => LevelFilter::ERROR,
        LogLevel::Warn => LevelFilter::WARN,
        LogLevel::Info => LevelFilter::INFO,
        LogLevel::Debug => LevelFilter::DEBUG,
        LogLevel::Trace => LevelFilter::TRACE,
    };
    let writer = Bounded {
        max: config.max_record_bytes,
    };
    let builder = tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(writer)
        .with_target(false);
    match config.format {
        LogFormat::Text => builder.try_init().is_ok(),
        LogFormat::Json => builder.json().try_init().is_ok(),
    }
}

/// Makes one [`Record`] writer per event.
#[derive(Debug, Clone, Copy)]
struct Bounded {
    max: usize,
}

impl<'a> MakeWriter<'a> for Bounded {
    type Writer = Record<io::Stderr>;

    fn make_writer(&'a self) -> Self::Writer {
        Record::new(io::stderr(), self.max)
    }
}

/// Writes at most `remaining` bytes of one record to `inner`; anything past
/// that is dropped and the record ends with ` [truncated]` and a newline.
/// Always reports the whole input as written, so the formatter never sees
/// an error for a record that was merely cut.
#[derive(Debug)]
pub(crate) struct Record<W: Write> {
    inner: W,
    remaining: usize,
    cut: bool,
}

impl<W: Write> Record<W> {
    pub(crate) fn new(inner: W, max: usize) -> Self {
        Self {
            inner,
            remaining: max,
            cut: false,
        }
    }
}

impl<W: Write> Write for Record<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self.cut {
            return Ok(buf.len());
        }
        if buf.len() <= self.remaining {
            self.inner.write_all(buf)?;
            self.remaining -= buf.len();
        } else {
            self.inner
                .write_all(buf.get(..self.remaining).unwrap_or_default())?;
            self.inner.write_all(MARKER)?;
            self.remaining = 0;
            self.cut = true;
        }
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_record_within_the_bound_is_written_whole() {
        let mut out = Vec::new();
        Record::new(&mut out, 16)
            .write_all(b"short line\n")
            .unwrap();
        assert_eq!(out, b"short line\n");
    }

    #[test]
    fn a_record_past_the_bound_is_cut_and_marked_once() {
        let mut out = Vec::new();
        let mut record = Record::new(&mut out, 4);
        record.write_all(b"abcdefgh").unwrap();
        record.write_all(b"more").unwrap();
        assert_eq!(out, b"abcd [truncated]\n");
    }
}
