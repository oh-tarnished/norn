//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The Parquet import reader: any file reads to its verified end or to an error,
//! holding at most a bounded row group.

#![no_main]

use std::fs::File;

use libfuzzer_sys::fuzz_target;
use norn_adapter_parquet::ParquetSource;
use norn_fuzz::scratch;
use norn_fuzz::source::{drain, panics_may_be_contained};

/// Smaller than other targets': a columnar file's every byte is a length.
const MAX_INPUT: usize = 256 << 10;
/// A bounded row group, as read and as converted.
const LIMIT: usize = 1 << 30;

fuzz_target!(|data: &[u8]| {
    panics_may_be_contained();
    if data.len() > MAX_INPUT {
        return;
    }
    let path = scratch("input.parquet");
    std::fs::write(&path, data).expect("a scratch file");
    let file = File::open(&path).expect("the scratch file");
    drain(ParquetSource::new(file), LIMIT);
});
