//! Recovery of a journal a crash left behind.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "common_ingest/mod.rs"]
mod common;

use common::{Dir, config, rig, sample};
use norn_runtime::ingest::AckLevel;

const MIB: u64 = 1 << 20;

#[test]
fn a_journal_left_by_a_crash_is_replayed_exactly_once() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(0..100, AckLevel::Buffered);
    r.ingest.close();
    let stream_dir = norn_storage::stream_dir(&dir.0, r.stream);
    let journal = stream_dir.join("journal");
    // A generation stored, its journal not yet deleted: rebuild that by hand.
    let mut writer = norn_storage::StreamWriter::open(&journal, r.stream, {
        let mut o = norn_storage::StorageOptions::from_config(&r.config);
        o.compression = norn_codec::compress::Compression::None;
        o
    })
    .unwrap();
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for n in [99u64, 500] {
        let s = sample(r.signal, n);
        bytes.extend_from_slice(&s.signal.get().to_le_bytes());
        bytes.extend_from_slice(&s.timestamp.as_nanos().to_le_bytes());
        bytes.extend_from_slice(&s.quality.code().to_le_bytes());
        bytes.extend_from_slice(&s.sequence.get().to_le_bytes());
        s.value.encode(&mut bytes).unwrap();
    }
    let frame = norn_core::Sample::new(
        norn_core::Timestamp::UNIX_EPOCH,
        norn_core::SignalId::new(0),
        norn_core::Value::Bytes(bytes),
        norn_core::Quality::GOOD,
        norn_core::Sequence::ZERO,
    );
    writer
        .append(&norn_storage::Chunk::from_samples(&[frame]).unwrap())
        .unwrap();
    writer.flush().unwrap();
    drop(writer);
    drop(r);
    let r = rig(&dir.0, config(64 * MIB), None);
    assert_eq!(r.stored(&dir.0).len(), 101);
    assert!(norn_storage::live_segments(&journal).unwrap().is_empty());
}

#[test]
fn stored_chunks_reach_the_index_and_a_restart_reuses_its_summary() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(0..100, AckLevel::Buffered);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let index = r.ingest.index().unwrap();
    while index.snapshot().tail_chunks(r.stream) == 0 {
        r.clock.advance(std::time::Duration::from_secs(7200));
        assert!(std::time::Instant::now() < deadline, "never published");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    r.ingest.close();
    let snapshot = r.ingest.index().unwrap().snapshot();
    assert_eq!(snapshot.tail_chunks(r.stream), 0, "closing sealed the tail");
    assert_eq!(snapshot.segments(r.stream).len(), 1);
    drop(snapshot);
    drop(r);
    let r = rig(&dir.0, config(64 * MIB), None);
    assert_eq!(
        r.ingest
            .index()
            .unwrap()
            .snapshot()
            .segments(r.stream)
            .len(),
        1
    );
}

#[test]
fn an_index_with_no_room_is_backpressure_and_the_samples_stay_safe() {
    let dir = Dir::new();
    let mut memory = norn_core::MemoryBudgets::default();
    memory.index_bytes = 1;
    let mut storage = norn_core::StorageConfig::default();
    storage.max_chunk_age = std::time::Duration::from_secs(3600);
    let cfg = norn_core::Config::builder()
        .memory(memory)
        .storage(storage)
        .build()
        .unwrap();
    let r = rig(&dir.0, cfg, None);
    r.batch(0..100, AckLevel::Buffered);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let mut n = 100;
    let refused = loop {
        r.clock.advance(std::time::Duration::from_secs(7200));
        let a = r.batch(n..n + 1, AckLevel::Buffered);
        n += 1;
        if a.reached.is_none() {
            break a;
        }
        assert!(std::time::Instant::now() < deadline, "never refused");
        std::thread::sleep(std::time::Duration::from_millis(5));
    };
    let reason = norn_runtime::ingest::Reason::MemoryExhausted;
    assert_eq!(refused.decision.reason, reason);
    assert!(refused.decision.retry_delay.is_some());
    r.ingest.close();
    assert_eq!(
        r.stored(&dir.0).len(),
        n as usize - 1,
        "every admitted sample"
    );
}

#[test]
fn a_compaction_replaces_its_inputs_in_the_index() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(0..100, AckLevel::Buffered);
    r.ingest.close();
    drop(r);
    let r = rig(&dir.0, config(64 * MIB), None);
    r.batch(100..200, AckLevel::Buffered);
    r.ingest.close();
    let stream_dir = norn_storage::stream_dir(&dir.0, r.stream);
    let opts = norn_storage::StorageOptions::from_config(&r.config);
    let inputs: Vec<_> = norn_storage::live_segments(&stream_dir)
        .unwrap()
        .into_iter()
        .map(|s| s.path)
        .collect();
    assert_eq!(inputs.len(), 2);
    let outcome = norn_storage::compact(&stream_dir, &inputs, &opts).unwrap();
    r.ingest.compaction_applied(r.stream, &outcome).unwrap();
    let snapshot = r.ingest.index().unwrap().snapshot();
    assert_eq!(snapshot.segments(r.stream).len(), 1);
}
