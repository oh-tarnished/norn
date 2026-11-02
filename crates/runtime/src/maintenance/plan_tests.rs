use std::time::Duration;

use norn_core::{SignalId, Timestamp};
use norn_index::SegmentRecord;
use norn_metadata::Retention;

use super::{RunLimits, next_run, retention};

const SECOND: i64 = 1_000_000_000;
const LIMITS: RunLimits = RunLimits {
    max_bytes: 1000,
    max_span_nanos: 100 * SECOND,
};

fn record(seq: u64, generation: u16, bytes: u64, from: i64, to: i64) -> SegmentRecord {
    SegmentRecord {
        first_seq: seq,
        last_seq: seq,
        generation,
        chunks: 1,
        samples: 10,
        file_len: bytes,
        min_ts: Timestamp::from_nanos(from * SECOND),
        max_ts: Timestamp::from_nanos(to * SECOND),
        min_signal: SignalId::new(2),
        max_signal: SignalId::new(2),
    }
}

fn seqs(run: &[SegmentRecord]) -> Vec<u64> {
    run.iter().map(|r| r.first_seq).collect()
}

#[test]
fn neighbours_within_the_limits_form_one_run() {
    let all = [record(0, 0, 100, 0, 10), record(1, 0, 100, 10, 20)];
    assert_eq!(seqs(&next_run(&all, LIMITS, None, &|_| false)), [0, 1]);
}

#[test]
fn a_run_stops_at_the_byte_limit() {
    let all = [record(0, 0, 900, 0, 10), record(1, 0, 900, 10, 20)];
    assert_eq!(seqs(&next_run(&all, LIMITS, None, &|_| false)), [0]);
}

#[test]
fn a_run_stops_at_the_span_limit() {
    let all = [record(0, 1, 10, 0, 90), record(1, 1, 10, 90, 180)];
    assert!(next_run(&all, LIMITS, None, &|_| false).is_empty());
}

#[test]
fn a_lone_compacted_segment_is_left_alone_so_planning_ends() {
    assert!(next_run(&[record(0, 1, 10, 0, 1)], LIMITS, None, &|_| false).is_empty());
}

#[test]
fn a_lone_segment_from_ingest_is_worth_compacting() {
    let run = next_run(&[record(0, 0, 10, 0, 1)], LIMITS, None, &|_| false);
    assert_eq!(seqs(&run), [0]);
}

#[test]
fn a_skipped_segment_splits_the_runs_around_it_and_is_never_an_input() {
    let all = [
        record(0, 1, 10, 0, 1),
        record(1, 0, 10, 1, 2),
        record(2, 1, 10, 2, 3),
        record(3, 1, 10, 3, 4),
    ];
    let run = next_run(&all, LIMITS, None, &|n| n.seq == 1);
    assert_eq!(seqs(&run), [2, 3]);
}

#[test]
fn an_interval_leaves_out_segments_it_does_not_reach() {
    let all = [
        record(0, 0, 10, 0, 1),
        record(1, 0, 10, 50, 51),
        record(2, 0, 10, 52, 53),
    ];
    let from = Timestamp::from_nanos(50 * SECOND);
    let to = Timestamp::from_nanos(60 * SECOND);
    assert_eq!(
        seqs(&next_run(&all, LIMITS, Some((from, to)), &|_| false)),
        [1, 2]
    );
}

#[test]
fn keeping_forever_removes_nothing() {
    let all = [record(0, 0, 10, 0, 1), record(1, 0, 10, 1, 2)];
    let newest = Some(Timestamp::from_nanos(1_000 * SECOND));
    assert!(retention(&all, Retention::Forever, newest).0.is_empty());
}

#[test]
fn a_segment_reaching_the_cutoff_stays_whole() {
    let all = [
        record(0, 0, 10, 0, 49),
        record(1, 0, 10, 40, 50),
        record(2, 0, 10, 50, 60),
        record(3, 0, 10, 90, 100),
    ];
    let policy = Retention::MaxAge(Duration::from_secs(50));
    let newest = Some(Timestamp::from_nanos(100 * SECOND));
    let (gone, cutoff) = retention(&all, policy, newest);
    assert_eq!(seqs(&gone), [0]);
    assert_eq!(cutoff, Some(Timestamp::from_nanos(50 * SECOND)));
}

#[test]
fn the_newest_segment_stays_even_when_past_the_limit() {
    let all = [record(0, 0, 10, 0, 1), record(1, 0, 10, 1, 2)];
    let policy = Retention::MaxAge(Duration::from_secs(1));
    let newest = Some(Timestamp::from_nanos(1_000 * SECOND));
    assert_eq!(seqs(&retention(&all, policy, newest).0), [0]);
}

#[test]
fn a_size_limit_takes_the_oldest_until_the_stream_fits() {
    let all = [
        record(0, 0, 100, 0, 1),
        record(1, 0, 100, 1, 2),
        record(2, 0, 100, 2, 3),
    ];
    assert_eq!(
        seqs(&retention(&all, Retention::MaxBytes(150), None).0),
        [0, 1]
    );
    assert_eq!(
        seqs(&retention(&all, Retention::MaxBytes(200), None).0),
        [0]
    );
    assert!(retention(&all, Retention::MaxBytes(300), None).0.is_empty());
}
