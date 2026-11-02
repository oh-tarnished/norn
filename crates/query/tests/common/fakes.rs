//! Stand-ins for the write path and the budget, and running a query to its end.

use std::collections::HashMap;
use std::ops::ControlFlow;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use norn_core::{Sample, SignalId, StreamId, Timestamp};
use norn_index::{IndexOptions, TemporalIndex};
use norn_query::{
    Engine, Page, Query, QueryError, QueryLimits, QueryMemory, QuerySpec, Refused, Results,
    TailSource,
};
use norn_storage::StorageOptions;

/// The write path's stand-in.
#[derive(Debug, Default)]
pub struct VecTail(pub Mutex<Vec<Sample>>);

impl TailSource for VecTail {
    fn scan(
        &self,
        _: StreamId,
        signal: SignalId,
        from: Timestamp,
        to: Timestamp,
        visit: &mut dyn FnMut(Sample) -> ControlFlow<()>,
    ) {
        for s in self.0.lock().unwrap().iter() {
            let inside = s.timestamp >= from && s.timestamp <= to;
            if s.signal == signal && inside && visit(s.clone()).is_break() {
                return;
            }
        }
    }

    fn newest_at_or_before(
        &self,
        _: StreamId,
        signal: SignalId,
        t: Timestamp,
    ) -> Option<Timestamp> {
        let all = self.0.lock().unwrap();
        let of = all.iter().filter(|s| s.signal == signal);
        of.map(|s| s.timestamp).filter(|x| *x <= t).max()
    }
}

/// A budget that remembers its peak, and can run a hook on the Nth acquire.
#[derive(Default)]
pub struct Counted {
    pub capacity: u64,
    pub used: AtomicU64,
    pub peak: AtomicU64,
    pub calls: AtomicU64,
    pub at_call: Mutex<HashMap<u64, Box<dyn FnOnce() + Send>>>,
}

impl std::fmt::Debug for Counted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Counted").finish_non_exhaustive()
    }
}

impl Counted {
    pub fn new(capacity: u64) -> Arc<Self> {
        Arc::new(Self {
            capacity,
            ..Self::default()
        })
    }
}

impl QueryMemory for Counted {
    fn acquire(&self, bytes: u64) -> Result<(), Refused> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let hook = self.at_call.lock().unwrap().remove(&call);
        if let Some(hook) = hook {
            hook();
        }
        let now = self.used.fetch_add(bytes, Ordering::SeqCst) + bytes;
        if now > self.capacity {
            self.used.fetch_sub(bytes, Ordering::SeqCst);
            return Err(Refused::Exhausted { requested: bytes });
        }
        self.peak.fetch_max(now, Ordering::SeqCst);
        Ok(())
    }

    fn try_acquire(&self, bytes: u64) -> Result<(), Refused> {
        self.acquire(bytes)
    }

    fn release(&self, bytes: u64) {
        self.used.fetch_sub(bytes, Ordering::SeqCst);
    }
}

pub struct Rig {
    pub index: Arc<TemporalIndex>,
    pub memory: Arc<Counted>,
    pub tail: Arc<VecTail>,
    pub engine: Arc<Engine>,
}

pub fn rig(root: &Path, capacity: u64, limits: QueryLimits) -> Rig {
    let (index, _) = TemporalIndex::open(root, IndexOptions::default()).unwrap();
    let index = Arc::new(index);
    let memory = Counted::new(capacity);
    let tail = Arc::new(VecTail::default());
    let engine = Arc::new(Engine::new(
        Arc::clone(&index),
        Arc::clone(&tail) as Arc<dyn TailSource>,
        Arc::clone(&memory) as Arc<dyn QueryMemory>,
        limits,
        StorageOptions::default(),
    ));
    Rig {
        index,
        memory,
        tail,
        engine,
    }
}

/// `(signal name, time, sequence, value bits)` of a raw result.
pub type Seen = (String, i64, u64, Vec<u8>);

pub fn seen(page: &Page) -> Vec<Seen> {
    let Results::Samples(items) = &page.results else {
        panic!("not samples");
    };
    let row = |(n, s): &(Arc<str>, Sample)| {
        let (t, q) = (s.timestamp.as_nanos(), s.sequence.get());
        let mut bits = Vec::new();
        s.value.encode(&mut bits).unwrap();
        (n.to_string(), t, q, bits)
    };
    items.iter().map(row).collect()
}

pub fn drain(query: &mut Query) -> Result<Vec<Page>, QueryError> {
    let mut out = Vec::new();
    while let Some(page) = query.next_page()? {
        out.push(page);
    }
    Ok(out)
}

pub fn all_seen(engine: &Arc<Engine>, spec: QuerySpec) -> Vec<Seen> {
    let mut query = engine.open(spec).unwrap();
    let mut out = Vec::new();
    // Page by page, like a real consumer: a page held is a page charged.
    while let Some(page) = query.next_page().unwrap() {
        out.extend(seen(&page));
    }
    out
}
