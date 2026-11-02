//! Write → read through real files, for every value kind.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, bytes_of, chunk, read_all, sealed, unsealed};
use norn_core::{Quality, Sample, Sequence, Severity, SignalId, Substatus, Timestamp, Value};
use norn_storage::{
    Chunk, SegmentReader, StorageError, StorageOptions, StreamWriter, list_segments, stream_dir,
};

fn one_kind(signal: u64, values: Vec<Value>) -> Vec<Sample> {
    values
        .into_iter()
        .enumerate()
        .map(|(i, v)| {
            let q = if i == 1 {
                Quality::new(Severity::Bad, Substatus::SensorFailure)
            } else {
                Quality::GOOD
            };
            Sample::new(
                Timestamp::from_nanos(1_000 + i as i64),
                SignalId::new(signal),
                v,
                q,
                Sequence::new(u64::MAX - i as u64),
            )
        })
        .collect()
}

fn every_kind() -> Vec<Vec<Sample>> {
    vec![
        one_kind(1, vec![Value::Bool(true), Value::Bool(false)]),
        one_kind(2, vec![Value::I32(i32::MIN), Value::I32(i32::MAX)]),
        one_kind(3, vec![Value::I64(i64::MIN), Value::I64(i64::MAX)]),
        one_kind(4, vec![Value::U32(0), Value::U32(u32::MAX)]),
        one_kind(5, vec![Value::U64(0), Value::U64(u64::MAX)]),
        one_kind(
            6,
            vec![Value::F32(f32::from_bits(0x7fc0_0001)), Value::F32(-0.0)],
        ),
        one_kind(
            7,
            vec![
                Value::F64(f64::from_bits(0x7ff8_0000_0000_0001)),
                Value::F64(-0.0),
            ],
        ),
        one_kind(
            8,
            vec![Value::String("déjà".into()), Value::String(String::new())],
        ),
        one_kind(9, vec![Value::Bytes(vec![0, 255]), Value::Bytes(vec![])]),
        one_kind(10, vec![Value::Enum(3), Value::Enum(u32::MAX)]),
    ]
}

#[test]
fn every_value_kind_survives_a_sealed_segment_bit_for_bit() {
    let dir = Scratch::new();
    let kinds = every_kind();
    let chunks: Vec<Chunk> = kinds
        .iter()
        .map(|s| Chunk::from_samples(s).unwrap())
        .collect();
    let seg = sealed(dir.path(), 0, &chunks);
    let want: Vec<Sample> = kinds.into_iter().flatten().collect();
    assert_eq!(seg.samples, want.len() as u64);
    assert_eq!(bytes_of(&read_all(&seg.path)), bytes_of(&want));
}

#[test]
fn an_unsealed_segment_is_readable_by_walking_its_frames() {
    let dir = Scratch::new();
    let path = unsealed(dir.path(), 0, &[chunk(1, 0, 10), chunk(2, 0, 10)]);
    let r = SegmentReader::open(&path, StorageOptions::default()).unwrap();
    assert!(!r.is_sealed());
    assert_eq!(r.chunks().len(), 2);
    assert_eq!(r.unverified_tail(), None);
    assert_eq!(read_all(&path).len(), 20);
}

#[test]
fn the_directory_lists_signal_and_time_range_without_decoding() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 100, 10), chunk(2, 500, 5)]);
    let r = SegmentReader::open(&seg.path, StorageOptions::default()).unwrap();
    assert!(r.is_sealed());
    let c = r.chunks()[1];
    assert_eq!(c.signal, SignalId::new(2));
    assert_eq!(c.min_ts, Timestamp::from_secs(500).unwrap());
    assert_eq!(c.max_ts, Timestamp::from_secs(504).unwrap());
    assert_eq!(c.count, 5);
}

#[test]
fn a_chunk_refuses_mixed_signals_mixed_kinds_and_no_samples() {
    assert!(Chunk::from_samples(&[]).is_err());
    let mut mixed = common::samples(1, 0, 1);
    mixed.extend(common::samples(2, 1, 1));
    assert!(matches!(
        Chunk::from_samples(&mixed),
        Err(StorageError::InvalidChunk(_))
    ));
    let mut kinds = common::samples(1, 0, 1);
    kinds.extend(one_kind(1, vec![Value::I32(1)]));
    assert!(matches!(
        Chunk::from_samples(&kinds),
        Err(StorageError::InvalidChunk(_))
    ));
}

#[test]
fn a_new_segment_never_overwrites_an_existing_one() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 3, &[chunk(1, 0, 1)]);
    let again =
        norn_storage::SegmentWriter::create(dir.path(), seg.header, StorageOptions::default());
    assert!(matches!(again, Err(StorageError::Io { .. })));
}

#[test]
fn a_stream_rolls_segments_on_the_size_limit() {
    let dir = Scratch::new();
    let path = stream_dir(dir.path(), common::STREAM);
    let mut opts = StorageOptions::default();
    opts.max_segment_bytes = 1024;
    let mut w = StreamWriter::open(&path, common::STREAM, opts).unwrap();
    let mut rolled = 0;
    for i in 0..40 {
        rolled += usize::from(w.append(&chunk(1, i * 100, 100)).unwrap().rolled.is_some());
    }
    w.close().unwrap();
    let segments = list_segments(&path).unwrap();
    assert!(rolled > 1);
    assert_eq!(segments.len(), rolled + 1);
    let total: usize = segments.iter().map(|s| read_all(&s.path).len()).sum();
    assert_eq!(total, 4000);
}

#[test]
fn a_stream_rolls_segments_on_the_span_of_sample_time() {
    let dir = Scratch::new();
    let mut opts = StorageOptions::default();
    opts.max_segment_span_nanos = 60_000_000_000;
    let mut w = StreamWriter::open(dir.path(), common::STREAM, opts).unwrap();
    for i in 0..10 {
        w.append(&chunk(1, i * 30, 30)).unwrap();
    }
    w.close().unwrap();
    assert!(list_segments(dir.path()).unwrap().len() >= 3);
}

#[test]
fn a_reopened_stream_continues_after_the_highest_sequence() {
    let dir = Scratch::new();
    sealed(dir.path(), 41, &[chunk(1, 0, 1)]);
    let mut w = StreamWriter::open(dir.path(), common::STREAM, StorageOptions::default()).unwrap();
    let a = w.append(&chunk(1, 1, 1)).unwrap();
    assert!(
        a.segment
            .to_string_lossy()
            .contains("000000000000002a-g0000")
    );
}

#[test]
fn a_chunk_above_the_sample_bound_is_refused_and_the_segment_stays_usable() {
    let dir = Scratch::new();
    let mut opts = StorageOptions::default();
    opts.max_chunk_samples = 5;
    let header = norn_storage::SegmentHeader::new(common::STREAM, 0);
    let mut w = norn_storage::SegmentWriter::create(dir.path(), header, opts).unwrap();
    assert!(matches!(
        w.append(&chunk(1, 0, 6)),
        Err(StorageError::TooLarge { .. })
    ));
    w.append(&chunk(1, 0, 5)).unwrap();
    assert_eq!(w.seal().unwrap().samples, 5);
}

#[test]
fn atomic_write_replaces_whole_files_and_leaves_no_temp() {
    let dir = Scratch::new();
    let p = dir.path().join("meta");
    norn_storage::atomic_write(&p, b"one").unwrap();
    norn_storage::atomic_write(&p, b"two!").unwrap();
    assert_eq!(std::fs::read(&p).unwrap(), b"two!");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
