//! A real write path on a temporary directory.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use norn_core::{
    Config, MemoryBudgets, Quality, Sample, Sequence, SignalId, StorageConfig, StreamId, Timestamp,
    Value, ValueKind,
};
use norn_metadata::{EntitySpec, Registry, SignalSpec, StreamSpec, Write};
use norn_runtime::ingest::{AckLevel, Admission, Batch, Ingest, SyncGate};
use norn_runtime::{Budget, Clock, FakeClock};
use norn_storage::{SegmentReader, StorageOptions, live_segments, stream_dir};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-ingest-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub struct Rig {
    pub ingest: Arc<Ingest>,
    pub registry: Arc<Registry>,
    pub budget: Budget,
    pub clock: FakeClock,
    pub stream: StreamId,
    pub signal: SignalId,
    pub config: Config,
}

/// `ingest_bytes` as asked; an hour-long chunk age, so a test decides when
/// things store by closing.
pub fn config(ingest_bytes: u64) -> Config {
    let mut memory = MemoryBudgets::default();
    memory.ingest_bytes = ingest_bytes;
    let mut storage = StorageConfig::default();
    storage.max_chunk_age = Duration::from_secs(3600);
    Config::builder()
        .memory(memory)
        .storage(storage)
        .build()
        .unwrap()
}

pub fn rig(root: &Path, config: Config, gate: Option<Arc<dyn SyncGate>>) -> Rig {
    let (registry, _) =
        Registry::open(&root.join("metadata"), config.limits(), config.metadata()).unwrap();
    let registry = Arc::new(registry);
    let now = Timestamp::from_nanos(1);
    let _ = registry.create_stream("s", StreamSpec::default(), now, Write::Commit);
    let _ = registry.create_entity("e", EntitySpec::default(), now, Write::Commit);
    let spec = SignalSpec::new(ValueKind::F64, "s".into());
    let _ = registry.create_signal("e", "t", spec, now, Write::Commit);
    let stream = registry.stream("s").unwrap().descriptor.id;
    let signal = registry.signal("e", "t").unwrap().descriptor.id;
    let clock = FakeClock::new(Timestamp::from_nanos(1_000));
    let shared: Arc<dyn Clock> = Arc::new(clock.clone());
    let budget = Budget::new(
        "ingest_bytes",
        config.memory().ingest_bytes,
        Arc::clone(&shared),
    );
    let mut ingest = Ingest::new(
        root.to_path_buf(),
        &config,
        Arc::clone(&registry),
        budget.clone(),
        shared,
    );
    if let Some(gate) = gate {
        ingest = ingest.sync_gate(gate);
    }
    ingest.recover().unwrap();
    Rig {
        ingest: Arc::new(ingest),
        registry,
        budget,
        clock,
        stream,
        signal,
        config,
    }
}

pub fn sample(signal: SignalId, n: u64) -> Sample {
    let time = Timestamp::from_nanos(i64::try_from(n).unwrap() * 1_000);
    Sample::new(
        time,
        signal,
        Value::F64(n as f64),
        Quality::GOOD,
        Sequence::new(n),
    )
}

impl Rig {
    pub fn batch(&self, range: std::ops::Range<u64>, level: AckLevel) -> Admission {
        let samples = range.map(|n| sample(self.signal, n)).collect();
        self.ingest.admit(Batch::new(self.stream, samples, level))
    }

    /// Everything stored, in file order.
    pub fn stored(&self, root: &Path) -> Vec<Sample> {
        let dir = stream_dir(root, self.stream);
        let opts = StorageOptions::from_config(&self.config);
        let mut out = Vec::new();
        for segment in live_segments(&dir).unwrap() {
            let mut reader = SegmentReader::open(&segment.path, opts).unwrap();
            let all = reader.samples(None, Timestamp::MIN, Timestamp::MAX);
            out.extend(all.map(Result::unwrap));
        }
        out
    }
}

/// A gate a test opens by hand, recording the order of events.
#[derive(Debug, Default)]
pub struct HeldGate {
    pub events: Mutex<Vec<&'static str>>,
    open: Mutex<bool>,
    changed: Condvar,
    pub fail: Mutex<bool>,
}

impl HeldGate {
    pub fn release(&self) {
        *self.open.lock().unwrap() = true;
        self.changed.notify_all();
    }
    pub fn fsynced(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| **e == "after")
            .count()
    }
    pub fn entered(&self) -> usize {
        self.events
            .lock()
            .unwrap()
            .iter()
            .filter(|e| **e == "before")
            .count()
    }
}

impl SyncGate for HeldGate {
    fn before_sync(&self) -> std::io::Result<()> {
        self.events.lock().unwrap().push("before");
        if *self.fail.lock().unwrap() {
            return Err(std::io::Error::other("injected fsync failure"));
        }
        Ok(())
    }

    /// Holds after the fsync returned: the last moment a wrong ack could leak.
    fn after_sync(&self) {
        self.events.lock().unwrap().push("after");
        let mut open = self.open.lock().unwrap();
        while !*open {
            open = self.changed.wait(open).unwrap();
        }
    }
}
