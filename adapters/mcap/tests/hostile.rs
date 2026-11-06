//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Files the fuzz targets found: each is an error, with nothing reserved for it.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::Cursor;

use norn_adapter::{AdapterError, SampleSource};
use norn_adapter_mcap::McapSource;

#[test]
fn a_string_length_past_its_record_is_refused_before_it_is_reserved() {
    let file = include_bytes!("data/hostile-string-length.mcap");
    let mut source = McapSource::new(Cursor::new(file));
    match source.catalog() {
        Err(AdapterError::Corrupt { detail, .. }) => {
            assert!(detail.contains("runs past"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

/// Debug builds panic in the crate on this; release builds wrap and run off the
/// end of the file. Either way, it must come out as an error.
#[test]
fn a_record_longer_than_its_chunk_is_an_error() {
    let file = include_bytes!("data/hostile-chunk-record.mcap");
    let mut source = McapSource::new(Cursor::new(file));
    let read = source.catalog().and_then(|_| {
        while source.next_batch()?.is_some() {}
        Ok(())
    });
    assert!(read.is_err(), "{read:?}");
}
