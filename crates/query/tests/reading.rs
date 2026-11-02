//! Only what a query needs is read, and damage is reported, never skipped.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::io::{Seek, SeekFrom, Write};

use common::{Scratch, all_seen, at, drain, rig, sample, seen, store, target};
use norn_core::{Sample, Value, ValueKind};
use norn_query::{Filter, Order, Output, QueryError, QueryLimits, QuerySpec, TimeRange};
use norn_storage::{SegmentReader, StorageOptions};

fn signals(n: u64) -> Vec<norn_query::Target> {
    (0..n).map(|s| target(s, ValueKind::F64)).collect()
}

fn spec(n: u64, output: Output) -> QuerySpec {
    QuerySpec {
        targets: signals(n),
        range: TimeRange::default(),
        filter: Filter::default(),
        output,
        resume: None,
    }
}

fn data(n: u64, each: i64) -> Vec<Sample> {
    let row = |s: u64| (0..each).map(move |t| sample(s, t * 10, Value::F64(t as f64)));
    (0..n).flat_map(row).collect()
}

#[test]
fn an_irrelevant_chunk_is_never_read() {
    let s = Scratch::new();
    store(&s.dir(), &[data(50, 1_000)], 100);
    let path = norn_storage::live_segments(&s.dir()).unwrap()[0]
        .path
        .clone();
    let chunks = SegmentReader::open(&path, StorageOptions::default()).unwrap();
    let wanted: Vec<_> = chunks
        .chunks()
        .iter()
        .filter(|c| c.signal.get() == 9 && c.overlaps(at(2_000), at(2_999)))
        .copied()
        .collect();
    assert_eq!(wanted.len(), 1);
    let frames: u64 = wanted.iter().map(|c| c.frame_len()).sum();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let query = QuerySpec {
        targets: vec![target(9, ValueKind::F64)],
        range: TimeRange {
            start: Some(at(2_000)),
            end: Some(at(3_000)),
        },
        ..spec(0, Output::Samples(Order::BySignal))
    };
    let mut query = r.engine.open(query).unwrap();
    assert_eq!(
        drain(&mut query).unwrap().iter().flat_map(seen).count(),
        100
    );
    let stats = query.stats();
    assert_eq!(stats.chunks_read, 1);
    assert_eq!(
        stats.bytes_read,
        48 + frames,
        "the header and one frame, no more"
    );
}

#[test]
fn a_chunk_failing_its_checksum_stops_the_query_and_says_which() {
    let s = Scratch::new();
    store(&s.dir(), &[data(1, 300)], 100);
    let path = norn_storage::live_segments(&s.dir()).unwrap()[0]
        .path
        .clone();
    let reader = SegmentReader::open(&path, StorageOptions::default()).unwrap();
    let damaged = reader.chunks()[1];
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.seek(SeekFrom::Start(damaged.offset + damaged.frame_len() - 1))
        .unwrap();
    file.write_all(&[0xff]).unwrap();
    drop(file);

    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let mut query = r
        .engine
        .open(spec(1, Output::Samples(Order::BySignal)))
        .unwrap();
    let mut delivered = 0;
    let error = loop {
        match query.next_page() {
            Ok(Some(page)) => delivered += seen(&page).len(),
            Ok(None) => panic!("the damage was skipped"),
            Err(e) => break e,
        }
    };
    let QueryError::Corrupt(found) = error else {
        panic!("{error}")
    };
    assert_eq!(found.offset, damaged.offset);
    assert_eq!(
        (found.min_ts, found.max_ts),
        (damaged.min_ts, damaged.max_ts)
    );
    assert!(
        delivered <= 100,
        "only samples before the damage: {delivered}"
    );

    // Around it, the rest reads: the error said where.
    let after = QuerySpec {
        range: TimeRange {
            start: Some(at(found.max_ts.as_nanos() + 1)),
            end: None,
        },
        ..spec(1, Output::Samples(Order::BySignal))
    };
    assert_eq!(all_seen(&r.engine, after).len(), 100);
}
