//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Telling a resend from new data, in bounded memory (docs/reference/storage.md, "Duplicates").

use std::collections::{HashMap, HashSet};
use std::hash::{BuildHasherDefault, Hasher};

use norn_core::{Sample, SignalId};

use crate::budget::{Budget, Reservation};

/// The largest window, in samples.
pub(crate) const MAX_WINDOW: usize = 262_144;
/// Below this the window is not worth having; the marks alone remain.
const MIN_WINDOW: usize = 256;
/// Memory one window entry costs: a ring slot plus a hash-set slot and spare.
const ENTRY_BYTES: u64 = 24;

/// A fingerprint is already well mixed, so hashing it again would be waste.
#[derive(Default)]
struct PassThrough(u64);

impl Hasher for PassThrough {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.rotate_left(8) ^ u64::from(*b);
        }
    }
    fn write_u64(&mut self, n: u64) {
        self.0 = n;
    }
}

fn mix(mut x: u64) -> u64 {
    x ^= x >> 33;
    x = x.wrapping_mul(0xff51_afd7_ed55_8ccd);
    x ^= x >> 33;
    x = x.wrapping_mul(0xc4ce_b9fe_1a85_ec53);
    x ^ (x >> 33)
}

/// A 64-bit fingerprint of the sample's identity: signal, time, sequence.
fn fingerprint(s: &Sample) -> u64 {
    let a = mix(s.signal.get()).wrapping_add(s.timestamp.as_nanos() as u64);
    mix(mix(a).wrapping_add(s.sequence.get()))
}

/// One stream's duplicate detector: a high-water mark per signal and a window of
/// recent fingerprints.
///
/// Memory: 24 bytes plus map overhead per signal, bounded by `max_signals`; the
/// window, reserved from the ingest budget when it is made and halved until it
/// fits. Concurrency: `&mut self`, one admission at a time.
#[derive(Debug)]
pub(crate) struct Dedup {
    marks: HashMap<SignalId, (i64, u64)>,
    seen: HashSet<u64, BuildHasherDefault<PassThrough>>,
    ring: Vec<u64>,
    next: usize,
    capacity: usize,
    _memory: Option<Reservation>,
}

impl std::fmt::Debug for PassThrough {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PassThrough")
    }
}

impl Dedup {
    /// A detector whose window takes at most an eighth of `budget`.
    pub(crate) fn new(budget: &Budget) -> Self {
        let share = budget.capacity() / 8 / ENTRY_BYTES;
        let mut capacity = usize::try_from(share).unwrap_or(MAX_WINDOW).min(MAX_WINDOW);
        let mut memory = None;
        while capacity >= MIN_WINDOW {
            if let Ok(r) = budget.try_reserve(capacity as u64 * ENTRY_BYTES) {
                memory = Some(r);
                break;
            }
            capacity /= 2;
        }
        if memory.is_none() {
            capacity = 0;
        }
        Self {
            marks: HashMap::new(),
            seen: HashSet::default(),
            ring: Vec::new(),
            next: 0,
            capacity,
            _memory: memory,
        }
    }

    /// How many recent samples the window holds: zero when it has no room.
    pub(crate) fn capacity(&self) -> usize {
        self.capacity
    }

    /// Records `s`, returning whether it is new. See docs/reference/storage.md for exactly what
    /// this guarantees.
    pub(crate) fn admit(&mut self, s: &Sample) -> bool {
        let key = (s.timestamp.as_nanos(), s.sequence.get());
        let print = fingerprint(s);
        let fresh = match self.marks.get_mut(&s.signal) {
            None => {
                self.marks.insert(s.signal, key);
                true
            }
            Some(mark) if key > *mark => {
                *mark = key;
                true
            }
            Some(mark) if key == *mark => false,
            Some(_) => !self.seen.contains(&print),
        };
        if fresh {
            self.remember(print);
        }
        fresh
    }

    fn remember(&mut self, print: u64) {
        if self.capacity == 0 || !self.seen.insert(print) {
            return;
        }
        if self.ring.len() < self.capacity {
            self.ring.push(print);
            return;
        }
        if let Some(slot) = self.ring.get_mut(self.next) {
            let old = std::mem::replace(slot, print);
            self.seen.remove(&old);
        }
        self.next = (self.next + 1) % self.capacity;
    }
}
