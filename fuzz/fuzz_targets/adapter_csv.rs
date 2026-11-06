//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The CSV import reader: any file reads to its verified end or to an error
//! naming the row, holding one batch and one bounded record.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use norn_adapter_csv::CsvSource;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::source::drain;

/// One batch (4 MiB of rows, as samples and signals) and one record.
const LIMIT: usize = 256 << 20;

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    drain(CsvSource::new(Cursor::new(data)), LIMIT);
});
