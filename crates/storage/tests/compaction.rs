//! Compaction preserves samples and survives a crash at each of its steps.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{STREAM, Scratch, bytes_of, chunk, read_all, sealed, small_chunks, unsealed};
use norn_core::Sample;
use norn_storage::{
    RecoveryMode, SegmentName, SegmentReader, StorageError, StorageOptions, compact, recover_stream,
};

fn opts() -> StorageOptions {
    StorageOptions::default()
}

/// Every sample of `signal`, in order: compaction must keep per-signal order.
fn of_signal(all: &[Sample], signal: u64) -> Vec<Sample> {
    all.iter()
        .filter(|s| s.signal.get() == signal)
        .cloned()
        .collect()
}

#[test]
fn compaction_preserves_every_sample_and_each_signals_order() {
    let dir = Scratch::new();
    let (inputs, all) = small_chunks(dir.path());
    let done = compact(dir.path(), &inputs, &opts()).unwrap();
    assert_eq!(done.chunks_before, 60);
    assert_eq!(done.output.chunks, 3, "one merged chunk per signal");
    assert_eq!(done.output.samples, all.len() as u64);
    assert!(done.output.bytes < done.bytes_before);
    assert!(inputs.iter().all(|p| !p.exists()));
    let got = read_all(&done.output.path);
    for signal in 1..=3 {
        assert_eq!(
            bytes_of(&of_signal(&got, signal)),
            bytes_of(&of_signal(&all, signal))
        );
    }
    let h = SegmentReader::open(&done.output.path, opts()).unwrap();
    assert_eq!(
        (
            h.header().first_seq,
            h.header().last_seq,
            h.header().generation
        ),
        (0, 1, 1)
    );
    assert!(
        recover_stream(dir.path(), &opts(), RecoveryMode::Repair)
            .unwrap()
            .is_clean()
    );
}

#[test]
fn merged_chunks_respect_the_sample_bound() {
    let dir = Scratch::new();
    let (inputs, all) = small_chunks(dir.path());
    let mut o = opts();
    o.max_chunk_samples = 75;
    let done = compact(dir.path(), &inputs, &o).unwrap();
    let r = SegmentReader::open(&done.output.path, o).unwrap();
    assert!(r.chunks().iter().all(|c| c.count <= 75));
    assert_eq!(done.output.samples, all.len() as u64);
}

#[test]
fn a_corrupt_input_aborts_compaction_before_the_commit() {
    let dir = Scratch::new();
    let (inputs, _) = small_chunks(dir.path());
    let mut b = fs::read(&inputs[0]).unwrap();
    b[100] ^= 1;
    fs::write(&inputs[0], &b).unwrap();
    assert!(matches!(
        compact(dir.path(), &inputs, &opts()),
        Err(StorageError::Corrupt(_))
    ));
    assert_eq!(fs::read(&inputs[0]).unwrap(), b);
    assert!(inputs[1].exists());
    assert_eq!(
        fs::read_dir(dir.path()).unwrap().count(),
        2,
        "no temp left behind"
    );
}

#[test]
fn compaction_refuses_what_it_could_not_safely_supersede() {
    let dir = Scratch::new();
    assert!(matches!(
        compact(dir.path(), &[], &opts()),
        Err(StorageError::CompactionRefused(_))
    ));
    let a = sealed(dir.path(), 0, &[chunk(1, 0, 5)]).path;
    let open = unsealed(dir.path(), 1, &[chunk(1, 5, 5)]);
    let c = sealed(dir.path(), 2, &[chunk(1, 10, 5)]).path;
    // An unsealed input.
    let r = compact(dir.path(), &[a.clone(), open], &opts());
    assert!(matches!(r, Err(StorageError::CompactionRefused(_))));
    // A hole: sequence 1 is live and would be superseded unmerged.
    let r = compact(dir.path(), &[a, c], &opts());
    assert!(matches!(r, Err(StorageError::CompactionRefused(_))));
}

#[test]
fn a_second_compaction_supersedes_the_first_outputs_generation() {
    let dir = Scratch::new();
    let (inputs, all) = small_chunks(dir.path());
    let one = compact(dir.path(), &inputs, &opts()).unwrap();
    let two = compact(dir.path(), std::slice::from_ref(&one.output.path), &opts()).unwrap();
    let name = SegmentName::parse(&two.output.path).unwrap();
    assert_eq!((name.seq, name.generation), (0, 2));
    assert_eq!(two.output.header.stream, STREAM);
    assert_eq!(read_all(&two.output.path).len(), all.len());
}
