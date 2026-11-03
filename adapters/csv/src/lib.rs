//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! CSV: one row per sample, exact for every value (SPEC §44, §45).
//!
//! # Layer contract
//!
//! - **Owns**: the long format below, [`CsvSink`] and [`CsvSource`].
//! - **Must not**: reach the daemon, which is `norn-adapter`'s job; hold more
//!   than one bounded batch, or one bounded record.
//! - **May import**: `norn-adapter`, `norn-protocol`; third-party: `crc32fast`.
//! - **Spans**: none.
//!
//! # Format
//!
//! RFC 4180 with `\n` line ends, UTF-8. The columns are fixed:
//!
//! ```text
//! # norn.csv 1 {"version":1,"streams":[...],"entities":[...],"signals":[...]}
//! time,signal,stream,type,value,severity,substatus,sequence
//! 2026-11-02T08:00:00.000000000Z,entities/a/signals/t,streams/s,double,21.5,GOOD,,0
//! # norn.end rows=1 crc32=<8 hex digits>
//! ```
//!
//! The `#` lines are what make an exported file checkable: the first carries the
//! catalog and promises the last, whose row count and CRC-32 of the row bytes
//! catch a file cut at a row boundary or a changed digit, neither of which CSV can
//! see. A file with no first `#` line (written by hand, or by another tool, or an
//! export with both lines removed after editing) is accepted, its signals
//! discovered row by row, and promises nothing. pandas reads an export with
//! `comment="#"` only if no string value holds a `#`; `skiprows=1, skipfooter=1`
//! is safe.
//!
//! Values, by `type`: `true`/`false`; decimal integers, `uint64` as unsigned;
//! floats as the shortest decimal that reads back exactly, `inf`, `-inf`, `NaN` for
//! the canonical quiet NaN and `nan:<hex bits>` for any other, so payloads survive;
//! `string` quoted when it must be; `bytes` as lower-case hex; `enum` as its number
//! (the symbol is display-only, and the daemon does not store it).

mod lines;
mod record;
mod row;
mod sink;
mod source;
mod value;

pub use sink::CsvSink;
pub use source::CsvSource;

/// The header row.
pub const HEADER: &str = "time,signal,stream,type,value,severity,substatus,sequence";

/// The start of the optional first line. Then the version, a space, the catalog.
pub(crate) const PREAMBLE: &str = "# norn.csv 1 ";

/// The start of the last line of a file with a preamble.
pub(crate) const TRAILER: &str = "# norn.end ";

/// The format's name, as errors give it.
pub(crate) const FORMAT: &str = "csv";
