//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The recovery marker `CLEAN`: any file vouches for nothing or for what it
//! lists, and what it lists survives being saved and loaded again.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::STREAM;
use norn_fuzz::{MAX_INPUT, measure, repaired, scratch, split};
use norn_storage::{CLEAN_FILE, CleanMarker};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let dir = scratch("clean");
    std::fs::create_dir_all(&dir).expect("a scratch directory");
    std::fs::write(dir.join(CLEAN_FILE), repaired(rest, selector & 1 == 1)).expect("a file");
    let (marker, peak) = measure(|| CleanMarker::load(&dir, STREAM));
    let limit = 16 * rest.len() + 4096;
    assert!(peak <= limit, "loading held {peak} bytes, above {limit}");
    let Some(marker) = marker else {
        return;
    };
    for s in marker.segments() {
        assert!(
            marker.vouches(s.name, s.bytes),
            "the marker disowned {:?}",
            s.name
        );
    }
    marker.save(&dir).expect("saving a loaded marker");
    let again = CleanMarker::load(&dir, STREAM).expect("a saved marker loads");
    assert_eq!(again.len(), marker.len(), "a marker lost segments");
    assert!(again.segments().all(|s| marker.segments().any(|t| s == t)));
});
