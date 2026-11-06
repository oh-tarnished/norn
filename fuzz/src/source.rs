//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The check every import adapter gets: read to the end or to an error.

use norn_adapter::SampleSource;

use crate::measure;

/// Reads `source` to its end, each step within `limit` bytes of heap. Returns
/// whether the file was accepted whole.
///
/// # Panics
/// When a read breaks the contract: that is the finding.
pub fn drain(mut source: impl SampleSource, limit: usize) -> bool {
    let (catalog, peak) = measure(|| source.catalog());
    assert!(peak <= limit, "reading the catalog held {peak} bytes");
    if catalog.is_err() {
        return false;
    }
    loop {
        let (batch, peak) = measure(|| source.next_batch());
        assert!(
            peak <= limit,
            "reading a batch held {peak} bytes, above {limit}"
        );
        match batch {
            Ok(Some(_)) => {}
            Ok(None) => return true,
            Err(_) => return false,
        }
    }
}

/// Silences the panic hook, so a panic this target's adapter contains is not taken
/// for a crash: libFuzzer's hook would abort before the adapter could catch it. One
/// that escapes still fails the run, through libFuzzer's own `catch_unwind`.
pub fn panics_may_be_contained() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    ONCE.call_once(|| std::panic::set_hook(Box::new(|_| {})));
}
