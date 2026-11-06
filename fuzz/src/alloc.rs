//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Counting the heap, so a target can assert a decoder stays inside its budget.
//!
//! This is the only `unsafe` in the fuzz crate, and the only way to observe
//! allocation from safe code: `GlobalAlloc` is an unsafe trait. Each method
//! forwards to [`System`] unchanged and then adjusts two counters. It is linked
//! into fuzz targets only; nothing Norn ships contains it.

#![allow(unsafe_code)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

/// The system allocator, counting live bytes and their peak.
///
/// Concurrency: the counters are process-wide, so a measurement includes any
/// other thread's allocations made during it; targets measure single-threaded
/// work.
#[derive(Debug)]
pub struct Counting;

fn grew(by: usize) {
    let live = LIVE.fetch_add(by, Ordering::Relaxed).saturating_add(by);
    PEAK.fetch_max(live, Ordering::Relaxed);
}

fn shrank(by: usize) {
    LIVE.fetch_sub(by, Ordering::Relaxed);
}

// SAFETY: every method forwards its arguments unchanged to `System`, which
// upholds `GlobalAlloc`'s contract; the counters touch no allocated memory.
unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller's `layout` obligations pass through unchanged.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            grew(layout.size());
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: `ptr` came from this allocator, which is `System`'s.
        unsafe { System.dealloc(ptr, layout) };
        shrank(layout.size());
    }

    /// Counted as the change in size: a realloc that moves briefly holds both
    /// blocks, but whether it moves is the system's choice, not the caller's.
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: `ptr` came from `System`, and the caller's obligations pass.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            match new_size.checked_sub(layout.size()) {
                Some(more) => grew(more),
                None => shrank(layout.size() - new_size),
            }
        }
        p
    }
}

/// Runs `f` and returns its result with the most heap it held at once beyond
/// what was live when it started.
pub fn measure<R>(f: impl FnOnce() -> R) -> (R, usize) {
    let base = LIVE.load(Ordering::Relaxed);
    PEAK.store(base, Ordering::Relaxed);
    let r = f();
    let peak = PEAK.load(Ordering::Relaxed);
    (r, peak.saturating_sub(base))
}
