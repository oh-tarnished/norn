//! A query sees every acknowledged sample, exactly once, across every flush.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "common_ingest/mod.rs"]
mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use common::{Dir, Rig, config, rig};
use norn_core::{Timestamp, Value, ValueKind};
use norn_runtime::ingest::{AckLevel, Ingest};
use norn_runtime::query::{
    Engine, Filter, Order, Output, QueryLimits, QueryMemory, QuerySpec, Results, TailSource,
    Target, TimeRange,
};
use norn_runtime::{Budget, Clock, SystemClock};
use norn_storage::StorageOptions;

const MIB: u64 = 1 << 20;

fn engine(r: &Rig) -> Arc<Engine> {
    let clock: Arc<dyn Clock> = Arc::new(SystemClock);
    let memory: Arc<dyn QueryMemory> = Arc::new(Budget::new("query_bytes", 64 * MIB, clock));
    let tail: Arc<Ingest> = Arc::clone(&r.ingest);
    Arc::new(Engine::new(
        Arc::clone(r.ingest.index().unwrap()),
        tail as Arc<dyn TailSource>,
        memory,
        QueryLimits::from_config(&r.config),
        StorageOptions::from_config(&r.config),
    ))
}

fn values(engine: &Arc<Engine>, r: &Rig) -> Vec<u64> {
    let spec = QuerySpec {
        targets: vec![Target {
            name: "entities/e/signals/t".into(),
            signal: r.signal,
            stream: r.stream,
            kind: ValueKind::F64,
        }],
        range: TimeRange::default(),
        filter: Filter::default(),
        output: Output::Samples(Order::BySignal),
        resume: None,
    };
    let mut query = engine.open(spec).unwrap();
    let mut out = Vec::new();
    while let Some(page) = query.next_page().unwrap() {
        let Results::Samples(list) = page.results else {
            panic!()
        };
        for (_, s) in list {
            let Value::F64(v) = s.value else { panic!() };
            out.push(v as u64);
        }
    }
    out
}

#[tokio::test(flavor = "multi_thread")]
async fn a_sample_acknowledged_persisted_is_queryable_at_once_and_counted() {
    let dir = Dir::new();
    let r = rig(&dir.0, config(64 * MIB), None);
    let engine = engine(&r);
    let mut admitted = r.batch(0..10, AckLevel::Persisted);
    admitted.completion.take().unwrap().await.unwrap().unwrap();
    assert_eq!(values(&engine, &r), (0..10).collect::<Vec<_>>());
    let stats = r.ingest.stats(r.stream);
    assert_eq!((stats.samples, stats.buffered), (10, 10));
    assert_eq!(
        stats.stored_bytes, 0,
        "the hour-long chunk age has not passed"
    );
    assert_eq!(
        stats.time_range,
        Some((Timestamp::from_nanos(0), Timestamp::from_nanos(9_000)))
    );
    r.ingest.close();
    let stats = r.ingest.stats(r.stream);
    assert_eq!((stats.samples, stats.buffered), (10, 0));
    assert!(stats.stored_bytes > 0);
    assert_eq!(values(&engine, &r), (0..10).collect::<Vec<_>>());
}

#[tokio::test(flavor = "multi_thread")]
async fn no_flush_loses_or_doubles_a_sample_for_a_query_running_through_it() {
    let dir = Dir::new();
    let r = Arc::new(rig(&dir.0, config(64 * MIB), None));
    let engine = engine(&r);
    let acked = Arc::new(AtomicU64::new(0));
    let done = Arc::new(AtomicBool::new(false));
    let reader = {
        let (r, engine, acked, done) = (
            Arc::clone(&r),
            Arc::clone(&engine),
            Arc::clone(&acked),
            Arc::clone(&done),
        );
        tokio::task::spawn_blocking(move || {
            let mut rounds = 0u32;
            while !done.load(Ordering::SeqCst) {
                let floor = acked.load(Ordering::SeqCst);
                let got = values(&engine, &r);
                let contiguous = got.iter().copied().eq(0..got.len() as u64);
                assert!(contiguous, "a gap or a double in {} rows", got.len());
                assert!(got.len() as u64 >= floor, "{} < acked {floor}", got.len());
                rounds += 1;
            }
            rounds
        })
    };
    const BATCH: u64 = 500;
    for n in 0..120u64 {
        let range = n * BATCH..(n + 1) * BATCH;
        let mut admitted = r.batch(range, AckLevel::Persisted);
        admitted.completion.take().unwrap().await.unwrap().unwrap();
        acked.store((n + 1) * BATCH, Ordering::SeqCst);
        if n % 7 == 3 {
            // Past the hour-long chunk age: the commit thread seals a generation.
            r.clock.advance(Duration::from_secs(3_601));
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
    done.store(true, Ordering::SeqCst);
    let rounds = reader.await.unwrap();
    assert!(rounds > 3, "the reader barely ran: {rounds}");
    let segments = r.ingest.index().unwrap().snapshot().tail_chunks(r.stream);
    assert!(segments > 0, "nothing was flushed, so nothing was tested");
    assert_eq!(r.ingest.stats(r.stream).samples, 120 * BATCH);
    r.ingest.close();
}
