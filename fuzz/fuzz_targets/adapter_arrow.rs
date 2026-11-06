//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The Arrow IPC import reader: any file reads to its verified end or to an
//! error, holding at most a bounded batch.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use norn_adapter_parquet::ArrowSource;
use norn_fuzz::source::{drain, panics_may_be_contained};

/// Smaller than other targets': a columnar file's every byte is a length.
const MAX_INPUT: usize = 256 << 10;
/// A bounded batch, as read and as converted.
const LIMIT: usize = 1 << 30;

fuzz_target!(|data: &[u8]| {
    panics_may_be_contained();
    if data.len() > MAX_INPUT {
        return;
    }
    drain(ArrowSource::new(Cursor::new(data)), LIMIT);
});
