//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The bounded page cache behind every lookup.

use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use norn_core::StreamId;
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

use crate::detail::DetailHead;

/// The item number meaning a segment's head rather than one of its pages.
const HEAD: u32 = u32::MAX;
/// What an entry costs beyond its value: the key twice, and map overhead. An
/// estimate, deliberately on the high side.
const ENTRY_OVERHEAD: u64 = 160;

/// A cache key. `file_len` is part of it because a segment name can in principle
/// recur after recovery quarantined the highest-numbered segment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct SegmentKey {
    pub(crate) stream: StreamId,
    pub(crate) segment: SegmentName,
    pub(crate) file_len: u64,
}

#[derive(Debug, Clone)]
enum Value {
    Head(Arc<DetailHead>),
    Page(Arc<[ChunkMeta]>),
}

#[derive(Debug)]
struct Entry {
    value: Value,
    bytes: u64,
    tick: u64,
}

#[derive(Debug, Default)]
struct Inner {
    map: HashMap<(SegmentKey, u32), Entry>,
    lru: BTreeMap<u64, (SegmentKey, u32)>,
    used: u64,
    tick: u64,
}

impl Inner {
    fn get(&mut self, key: (SegmentKey, u32)) -> Option<Value> {
        self.tick += 1;
        let tick = self.tick;
        let e = self.map.get_mut(&key)?;
        self.lru.remove(&e.tick);
        e.tick = tick;
        self.lru.insert(tick, key);
        Some(e.value.clone())
    }

    fn evict_to(&mut self, capacity: u64) {
        while self.used > capacity {
            let Some((_, key)) = self.lru.pop_first() else {
                return;
            };
            if let Some(e) = self.map.remove(&key) {
                self.used -= e.bytes;
            }
        }
    }
}

/// Least-recently-used heads and pages, at most `capacity` bytes by the entries'
/// accounted size.
///
/// An entry larger than the capacity is not kept; the caller still has its
/// `Arc`, so the bound can only be passed by what lookups are using right now,
/// one page per in-flight lookup.
///
/// Concurrency: `Sync`. One mutex, held for one map operation, never across I/O.
/// Two threads missing the same page both read it, which is harmless.
/// Performance: O(log n) per access.
#[derive(Debug, Default)]
pub(crate) struct PageCache {
    inner: Mutex<Inner>,
    capacity: AtomicU64,
}

impl PageCache {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // No user code runs under the lock and every step keeps the maps
        // consistent, so a poisoned state is still sound.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the capacity, evicting down to it.
    pub(crate) fn set_capacity(&self, capacity: u64) {
        self.capacity.store(capacity, Ordering::Release);
        self.lock().evict_to(capacity);
    }

    /// Bytes held.
    pub(crate) fn used(&self) -> u64 {
        self.lock().used
    }

    /// Drops everything.
    pub(crate) fn clear(&self) {
        let mut inner = self.lock();
        inner.map.clear();
        inner.lru.clear();
        inner.used = 0;
    }

    /// The head of `key`, if cached.
    pub(crate) fn head(&self, key: SegmentKey) -> Option<Arc<DetailHead>> {
        match self.lock().get((key, HEAD))? {
            Value::Head(h) => Some(h),
            Value::Page(_) => None,
        }
    }

    /// Page `page` of `key`, if cached.
    pub(crate) fn page(&self, key: SegmentKey, page: u32) -> Option<Arc<[ChunkMeta]>> {
        match self.lock().get((key, page))? {
            Value::Page(p) => Some(p),
            Value::Head(_) => None,
        }
    }

    /// Caches the head of `key`.
    pub(crate) fn put_head(&self, key: SegmentKey, head: Arc<DetailHead>) {
        let bytes = head.memory_bytes();
        self.put((key, HEAD), Value::Head(head), bytes);
    }

    /// Caches page `page` of `key`.
    pub(crate) fn put_page(&self, key: SegmentKey, page: u32, chunks: Arc<[ChunkMeta]>) {
        let bytes = (chunks.len() * size_of::<ChunkMeta>()) as u64;
        self.put((key, page), Value::Page(chunks), bytes);
    }

    fn put(&self, key: (SegmentKey, u32), value: Value, bytes: u64) {
        let capacity = self.capacity.load(Ordering::Acquire);
        let bytes = bytes + ENTRY_OVERHEAD;
        if bytes > capacity {
            return;
        }
        let mut inner = self.lock();
        inner.tick += 1;
        let tick = inner.tick;
        let entry = Entry { value, bytes, tick };
        if let Some(old) = inner.map.insert(key, entry) {
            inner.lru.remove(&old.tick);
            inner.used -= old.bytes;
        }
        inner.lru.insert(tick, key);
        inner.used += bytes;
        inner.evict_to(capacity);
    }
}
