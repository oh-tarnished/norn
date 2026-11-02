//! Held answers, so that every page of one state describes the same state.

use std::collections::VecDeque;
use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};

use norn_core::Timestamp;

use crate::reader::{SignalState, State};

/// Answers held at once. Picked: paging is a burst by one client, and an answer
/// that fits one page is never pinned. The oldest is evicted past this.
const MAX_PINS: usize = 16;
/// How long a pin outlives its last use, in nanoseconds. Picked: a client reads its
/// next page in milliseconds, and this forgives one that stops to think.
const IDLE_NANOS: i64 = 60_000_000_000;

/// Names one pinned answer. Unguessable it is not: it is local IPC and not a
/// capability.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinId(pub u64);

impl fmt::Display for PinId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug)]
struct Pin {
    id: u64,
    print: u64,
    used: Timestamp,
    state: Arc<State>,
}

#[derive(Debug, Default)]
struct Held {
    next: u64,
    pins: VecDeque<Pin>,
}

impl Held {
    fn expire(&mut self, now: Timestamp) {
        let live = |p: &Pin| now.as_nanos().saturating_sub(p.used.as_nanos()) < IDLE_NANOS;
        self.pins.retain(live);
    }
}

/// A bounded set of answers being read page by page.
///
/// A sample can arrive between two pages, and no stored sample says when it did, so
/// a later page could not be recomputed to agree with an earlier one. The answer is
/// therefore held, whole, charged to the query budget, until its last page is read,
/// it goes idle, or newer pins evict it. A token for one that has gone fails
/// [`Pins::get`], and the client starts again. Nothing survives a restart.
///
/// Time arrives as an argument. Concurrency: `Send + Sync`; one short lock a call.
/// Memory: at most 16 answers. Failure: none of its own.
#[derive(Debug, Default)]
pub struct Pins {
    held: Mutex<Held>,
}

impl Pins {
    /// No answers held.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Holds `state` for a request whose fingerprint is `print`.
    pub fn pin(&self, state: Arc<State>, print: u64, now: Timestamp) -> PinId {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.expire(now);
        while held.pins.len() >= MAX_PINS {
            held.pins.pop_front();
        }
        let id = held.next;
        held.next += 1;
        held.pins.push_back(Pin {
            id,
            print,
            used: now,
            state,
        });
        PinId(id)
    }

    /// The answer `id`, if it is still held and was made for `print`.
    #[must_use]
    pub fn get(&self, id: PinId, print: u64, now: Timestamp) -> Option<Arc<State>> {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.expire(now);
        let pin = held.pins.iter_mut().find(|p| p.id == id.0)?;
        if pin.print != print {
            return None;
        }
        pin.used = now;
        Some(Arc::clone(&pin.state))
    }

    /// Lets go of `id`: its last page has been read.
    pub fn release(&self, id: PinId) {
        let mut held = self.held.lock().unwrap_or_else(PoisonError::into_inner);
        held.pins.retain(|p| p.id != id.0);
    }
}

/// How many entries from `start` fit a page of `limit` entries and `bytes` bytes.
/// At least one, while any remain, however large it is.
#[must_use]
pub fn page_length(entries: &[SignalState], limit: usize, bytes: u64) -> usize {
    let mut used = 0u64;
    let mut count = 0;
    for entry in entries.iter().take(limit) {
        used = used.saturating_add(State::encoded_bytes(entry));
        if used > bytes && count > 0 {
            break;
        }
        count += 1;
    }
    count
}
