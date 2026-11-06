//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The MCAP import reader: any file reads to its verified end or to an error,
//! holding one record of bounded length and one batch.

#![no_main]

use std::io::Cursor;

use libfuzzer_sys::fuzz_target;
use norn_adapter_mcap::McapSource;
use norn_fuzz::MAX_INPUT;
use norn_fuzz::source::{drain, panics_may_be_contained};

/// One record at its limit, as read and as decoded, and one batch.
const LIMIT: usize = 512 << 20;

fuzz_target!(|data: &[u8]| {
    panics_may_be_contained();
    if data.len() > MAX_INPUT {
        return;
    }
    drain(McapSource::new(Cursor::new(data)), LIMIT);
});
