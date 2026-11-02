//! Chunks written but not sealed, and readers running beside the updater.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use common::{STREAM, Scratch, chunk, keys, open, plan, sealed, secs};
use norn_core::SignalId;
use norn_index::{PlanRequest, SignalFilter};
use norn_storage::{ChunkFile, SegmentName, StorageOptions, StreamWriter};

#[test]
fn a_flushed_chunk_is_visible_and_readable_before_the_seal() {
    let s = Scratch::new();
    let index = open(&s);
    let mut w = StreamWriter::open(&s.dir(), STREAM, StorageOptions::default()).unwrap();
    let appended = w.append(&chunk(1, 0, 10, 1)).unwrap();
    w.flush().unwrap();
    let name = SegmentName::parse(&appended.segment).unwrap();
    index
        .publish_chunks(STREAM, name, &[appended.chunk])
        .unwrap();

    let snap = index.snapshot();
    let got = plan(&snap, PlanRequest::new(secs(0), secs(5)));
    assert_eq!(got.len(), 1);
    assert!(!got[0].sealed);
    let mut f = ChunkFile::open(&snap.path_of(&got[0]), StorageOptions::default()).unwrap();
    assert_eq!(f.read_chunk(&got[0].chunk).unwrap().len(), 10);
    let near = snap
        .nearest_at_or_before(SignalId::new(1), secs(100))
        .unwrap();
    assert_eq!(near.unwrap().chunk, appended.chunk);
}

#[test]
fn sealing_swaps_the_tail_for_the_record_in_one_step() {
    let s = Scratch::new();
    let index = open(&s);
    let mut w = StreamWriter::open(&s.dir(), STREAM, StorageOptions::default()).unwrap();
    let (mut metas, mut segment) = (Vec::new(), None);
    for i in 0..700 {
        let a = w.append(&chunk(i % 5, i as i64 * 2, 2, 1)).unwrap();
        segment = Some(SegmentName::parse(&a.segment).unwrap());
        metas.push(a.chunk);
    }
    w.flush().unwrap();
    index
        .publish_chunks(STREAM, segment.unwrap(), &metas)
        .unwrap();
    let before = plan(&index.snapshot(), PlanRequest::everything());
    assert_eq!(before.len(), 700);
    assert_eq!(index.snapshot().tail_chunks(STREAM), 700);

    let summary = w.close().unwrap().unwrap();
    index.segment_sealed(STREAM, &summary).unwrap();
    let snap = index.snapshot();
    assert_eq!(snap.tail_chunks(STREAM), 0);
    let after = plan(&snap, PlanRequest::everything());
    assert!(after.iter().all(|l| l.sealed));
    assert_eq!(keys(&after), keys(&before), "never missing, never twice");
}

#[test]
fn publishing_twice_or_late_changes_nothing() {
    let s = Scratch::new();
    let index = open(&s);
    let mut w = common::writer(&s.dir(), 0);
    let meta = w.append(&chunk(1, 0, 10, 1)).unwrap();
    let name = SegmentName::of(w.header());
    index.publish_chunks(STREAM, name, &[meta]).unwrap();
    index.publish_chunks(STREAM, name, &[meta]).unwrap();
    assert_eq!(index.snapshot().tail_chunks(STREAM), 1);
    let summary = w.seal().unwrap();
    index.segment_sealed(STREAM, &summary).unwrap();
    index.publish_chunks(STREAM, name, &[meta]).unwrap();
    assert_eq!(index.snapshot().tail_chunks(STREAM), 0);
}

#[test]
fn a_signal_filter_applies_to_the_tail_and_merges_with_sealed_segments() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(1, 0, 5, 1), chunk(2, 0, 5, 1)]);
    let index = open(&s);
    let mut w = common::writer(&s.dir(), 1);
    let metas = [
        w.append(&chunk(1, 2, 5, 1)).unwrap(),
        w.append(&chunk(2, 2, 5, 1)).unwrap(),
    ];
    w.flush().unwrap();
    index
        .publish_chunks(STREAM, SegmentName::of(w.header()), &metas)
        .unwrap();
    let request = PlanRequest::everything().signals(SignalFilter::one(SignalId::new(1)));
    let got = plan(&index.snapshot(), request);
    assert_eq!(got.len(), 2);
    assert!(got[0].sealed && !got[1].sealed);
}

#[test]
fn readers_keep_a_consistent_view_while_the_updater_runs() {
    let s = Scratch::new();
    let index = Arc::new(open(&s));
    let stop = Arc::new(AtomicBool::new(false));
    let readers: Vec<_> = (0..4)
        .map(|_| {
            let (index, stop) = (Arc::clone(&index), Arc::clone(&stop));
            std::thread::spawn(move || {
                let mut seen = 0;
                while !stop.load(Ordering::Relaxed) {
                    let got = plan(&index.snapshot(), PlanRequest::everything());
                    assert!(got.windows(2).all(|w| w[0] < w[1]), "order under updates");
                    assert!(got.len() >= seen, "time never runs backwards");
                    seen = got.len();
                }
            })
        })
        .collect();
    for seq in 0..60u64 {
        let chunks: Vec<_> = (0..3).map(|n| chunk(n, seq as i64 * 100, 5, 1)).collect();
        let summary = sealed(&s.dir(), seq, &chunks);
        index.segment_sealed(STREAM, &summary).unwrap();
    }
    stop.store(true, Ordering::Relaxed);
    readers.into_iter().for_each(|r| r.join().unwrap());
    assert_eq!(
        plan(&index.snapshot(), PlanRequest::everything()).len(),
        180
    );
}
