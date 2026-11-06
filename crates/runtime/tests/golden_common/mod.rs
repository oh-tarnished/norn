//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The golden stores: what they hold, how they were written, and how to open one.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use norn_core::{
    Config, Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value, ValueKind,
};
use norn_metadata::{EntitySpec, Registry, SignalSpec, StreamSpec, Write};
use norn_runtime::ingest::{AckLevel, Batch, Ingest, IngestRecovery};
use norn_runtime::{Budget, Clock, FakeClock};
use norn_storage::{SegmentReader, StorageOptions, live_segments, stream_dir};

pub const KINDS: [(&str, ValueKind); 5] = [
    ("temperature", ValueKind::F64),
    ("count", ValueKind::I64),
    ("open", ValueKind::Bool),
    ("label", ValueKind::String),
    ("blob", ValueKind::Bytes),
];
pub const STREAMS: [&str; 2] = ["plant", "merged"];
/// Samples each signal gets in each session of writing.
pub const PER_SESSION: u64 = 40;
const INGEST_BYTES: u64 = 64 << 20;

/// Where the checked-in stores live.
pub fn golden_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/golden/v1")
}

/// The value signal number `kind` has at sample `n`.
pub fn value(kind: ValueKind, n: u64) -> Value {
    match kind {
        ValueKind::F64 => Value::F64(n as f64 * 0.5),
        ValueKind::I64 => Value::I64(i64::try_from(n).unwrap() - 7),
        ValueKind::Bool => Value::Bool(n.is_multiple_of(3)),
        ValueKind::String => Value::String(format!("label {n}")),
        _ => Value::Bytes(vec![u8::try_from(n % 251).unwrap(); 8]),
    }
}

/// One signal's sample `n`. Its time and sequence depend only on `n`.
pub fn sample(signal: SignalId, kind: ValueKind, n: u64) -> Sample {
    let at = Timestamp::from_nanos(1_700_000_000_000_000_000 + i64::try_from(n).unwrap() * 1_000);
    Sample::new(at, signal, value(kind, n), Quality::GOOD, Sequence::new(n))
}

/// An open database: the registry, and the write path after recovery.
pub struct Open {
    pub registry: Arc<Registry>,
    pub ingest: Arc<Ingest>,
    pub recovery: IngestRecovery,
    pub config: Config,
}

pub fn open(root: &Path) -> Open {
    let mut memory = norn_core::MemoryBudgets::default();
    memory.ingest_bytes = INGEST_BYTES;
    let config = Config::builder().memory(memory).build().unwrap();
    let dir = root.join("metadata");
    let (registry, _) = Registry::open(&dir, config.limits(), config.metadata()).unwrap();
    let registry = Arc::new(registry);
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(Timestamp::from_nanos(1_000)));
    let budget = Budget::new("ingest_bytes", INGEST_BYTES, Arc::clone(&clock));
    let ingest = Ingest::new(
        root.to_path_buf(),
        &config,
        Arc::clone(&registry),
        budget,
        clock,
    );
    let recovery = ingest.recover().unwrap();
    Open {
        registry,
        ingest: Arc::new(ingest),
        recovery,
        config,
    }
}

impl Open {
    pub fn stream(&self, name: &str) -> StreamId {
        self.registry.stream(name).unwrap().descriptor.id
    }

    pub fn signal(&self, stream: &str, signal: &str) -> SignalId {
        self.registry.signal(stream, signal).unwrap().descriptor.id
    }

    /// Everything `stream` has in its live segments, sorted, as `(signal, n, value)`.
    pub fn stored(&self, root: &Path, stream: StreamId) -> Vec<(SignalId, u64, Value)> {
        let opts = StorageOptions::from_config(&self.config);
        let mut out = Vec::new();
        for segment in live_segments(&stream_dir(root, stream)).unwrap() {
            let mut reader = SegmentReader::open(&segment.path, opts).unwrap();
            for s in reader.samples(None, Timestamp::MIN, Timestamp::MAX) {
                let s = s.unwrap();
                out.push((s.signal, s.sequence.get(), s.value));
            }
        }
        out.sort_by_key(|(signal, n, _)| (*signal, *n));
        out
    }

    /// What `stored` must return for a stream given `sessions` of writing.
    pub fn expected(&self, stream: &str, sessions: u64) -> Vec<(SignalId, u64, Value)> {
        let mut out = Vec::new();
        for (name, kind) in KINDS {
            let signal = self.signal(stream, name);
            for n in 0..sessions * PER_SESSION {
                out.push((signal, n, value(kind, n)));
            }
        }
        out.sort_by_key(|(signal, n, _)| (*signal, *n));
        out
    }

    /// Writes session number `session` of `stream` and waits for it to be
    /// durable. The thread blocks; nothing else waits on it.
    pub fn write_session(&self, stream: &str, session: u64) {
        let id = self.stream(stream);
        let mut samples = Vec::new();
        for (name, kind) in KINDS {
            let signal = self.signal(stream, name);
            for n in session * PER_SESSION..(session + 1) * PER_SESSION {
                samples.push(sample(signal, kind, n));
            }
        }
        let batch = Batch::new(id, samples, AckLevel::Durable);
        let completion = self.ingest.admit(batch).completion.unwrap();
        completion.blocking_recv().unwrap().unwrap();
    }
}

/// Creates the catalogue: each stream under an entity of the same name.
pub fn declare(open: &Open) {
    let now = Timestamp::from_nanos(1);
    for stream in STREAMS {
        let (spec, write) = (StreamSpec::default(), Write::Commit);
        open.registry
            .create_stream(stream, spec, now, write)
            .unwrap();
        let spec = EntitySpec::default();
        open.registry
            .create_entity(stream, spec, now, Write::Commit)
            .unwrap();
        for (name, kind) in KINDS {
            let spec = SignalSpec::new(kind, stream.into());
            let made = open
                .registry
                .create_signal(stream, name, spec, now, Write::Commit);
            made.unwrap();
        }
    }
}

/// Copies a store, leaving out the lock files a running build holds, and empty
/// directories, which git cannot hold: a checkout has none.
pub fn copy_store(from: &Path, to: &Path) {
    for entry in fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let (path, target) = (entry.path(), to.join(entry.file_name()));
        if path.is_dir() {
            copy_store(&path, &target);
        } else if entry.file_name() != "LOCK" {
            fs::create_dir_all(to).unwrap();
            fs::copy(&path, &target).unwrap();
        }
    }
}
