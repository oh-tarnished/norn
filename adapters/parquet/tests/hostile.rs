//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Files the fuzz targets found: each is an error, never a panic.
#![allow(clippy::unwrap_used, clippy::panic)]

use std::io::Cursor;

use norn_adapter::{AdapterError, SampleSource};
use norn_adapter_parquet::ArrowSource;

fn read(mut source: impl SampleSource) -> Result<(), AdapterError> {
    source.catalog()?;
    while source.next_batch()?.is_some() {}
    Ok(())
}

#[test]
fn an_arrow_buffer_length_past_the_file_is_corruption_not_a_panic() {
    let file = include_bytes!("data/hostile-buffer-length.arrow");
    match read(ArrowSource::new(Cursor::new(file))) {
        Err(AdapterError::Corrupt { detail, .. }) => {
            assert!(detail.contains("gave up"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn an_arrow_block_past_the_file_is_refused_before_it_is_reserved() {
    let file = include_bytes!("data/hostile-block-length.arrow");
    match read(ArrowSource::new(Cursor::new(file))) {
        Err(AdapterError::Truncated { detail, .. }) => {
            assert!(detail.contains("past the end"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}
