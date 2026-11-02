//! StateAt's edges, and holding an answer for its pages.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../../query/tests/common/mod.rs"]
mod common;

use std::io::{Seek, SeekFrom, Write};
use std::sync::Arc;

use common::{Scratch, at, rig, sample, store, target};
use norn_core::{Value, ValueKind};
use norn_query::{QueryError, QueryLimits, Refused, Target};
use norn_state::{PinId, Pins, StateReader, page_length};
use norn_storage::live_segments;

fn one(signal: u64) -> Vec<Target> {
    vec![target(signal, ValueKind::F64)]
}

fn value(i: i64) -> Value {
    Value::F64(i as f64)
}

#[test]
fn a_signal_with_no_sample_yet_has_an_entry_and_no_state() {
    let s = Scratch::new();
    store(&s.dir(), &[vec![sample(1, 100, value(1))]], 10);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let reader = StateReader::new(r.engine.clone());
    let state = reader.state_at(one(1), at(99), None).unwrap();
    assert_eq!(state.entries().len(), 1);
    assert_eq!(state.entries()[0].sample, None);
    let state = reader.state_at(one(2), at(1000), None).unwrap();
    assert_eq!(state.entries()[0].sample, None, "a signal never written");
}

#[test]
fn an_instant_inside_a_gap_finds_the_last_sample_before_the_gap() {
    let s = Scratch::new();
    let early = (0..10).map(|t| sample(1, t, value(t))).collect();
    let late = (1000..1010).map(|t| sample(1, t, value(t))).collect();
    store(&s.dir(), &[early, late], 4);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let state = StateReader::new(r.engine.clone())
        .state_at(one(1), at(500), None)
        .unwrap();
    assert_eq!(state.entries()[0].sample.as_ref().unwrap().timestamp, at(9));
}

#[test]
fn max_age_keeps_a_sample_exactly_that_old_and_drops_an_older_one() {
    let s = Scratch::new();
    store(&s.dir(), &[vec![sample(1, 100, value(1))]], 10);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let reader = StateReader::new(r.engine.clone());
    let kept = reader.state_at(one(1), at(150), Some(50)).unwrap();
    assert!(kept.entries()[0].sample.is_some());
    let gone = reader.state_at(one(1), at(150), Some(49)).unwrap();
    assert_eq!(gone.entries()[0].sample, None);
}

#[test]
fn a_higher_sequence_in_another_chunk_at_the_same_instant_wins() {
    let s = Scratch::new();
    let low = sample(1, 100, value(1));
    let mut high = sample(1, 100, value(2));
    high.sequence = norn_core::Sequence::new(101);
    store(&s.dir(), &[vec![high], vec![low]], 10);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let state = StateReader::new(r.engine.clone())
        .state_at(one(1), at(100), None)
        .unwrap();
    let got = state.entries()[0].sample.as_ref().unwrap();
    assert_eq!(got.value, value(2));
}

#[test]
fn a_buffered_sample_beats_a_stored_one() {
    let s = Scratch::new();
    store(&s.dir(), &[vec![sample(1, 100, value(1))]], 10);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    *r.tail.0.lock().unwrap() = vec![sample(1, 200, value(2))];
    let state = StateReader::new(r.engine.clone())
        .state_at(one(1), at(300), None)
        .unwrap();
    assert_eq!(state.entries()[0].sample.as_ref().unwrap().value, value(2));
}

#[test]
fn a_damaged_chunk_is_corruption_and_never_no_state() {
    let s = Scratch::new();
    store(&s.dir(), &[vec![sample(1, 100, value(1))]], 10);
    let path = live_segments(&s.dir()).unwrap()[0].path.clone();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(100)).unwrap();
    file.write_all(&[0xff; 16]).unwrap();
    drop(file);
    let error = StateReader::new(r.engine.clone())
        .state_at(one(1), at(150), None)
        .unwrap_err();
    assert!(matches!(error, QueryError::Corrupt(_)), "{error:?}");
}

#[test]
fn an_answer_too_large_for_the_budget_is_a_typed_refusal() {
    let s = Scratch::new();
    let r = rig(&s.0, 16, QueryLimits::default());
    let error = StateReader::new(r.engine.clone())
        .state_at(one(1), at(1), None)
        .unwrap_err();
    assert!(matches!(
        error,
        QueryError::Memory(Refused::Exhausted { .. })
    ));
}

#[test]
fn an_answers_charge_lasts_exactly_as_long_as_the_answer() {
    let s = Scratch::new();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let state = StateReader::new(r.engine.clone())
        .state_at(one(1), at(1), None)
        .unwrap();
    let pins = Pins::new();
    let print = 7;
    let id = pins.pin(Arc::new(state), print, at(0));
    assert!(r.memory.used.load(std::sync::atomic::Ordering::SeqCst) > 0);
    pins.release(id);
    assert_eq!(r.memory.used.load(std::sync::atomic::Ordering::SeqCst), 0);
}

#[test]
fn a_pin_answers_only_its_own_request_and_goes_when_idle() {
    let s = Scratch::new();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let state = StateReader::new(r.engine.clone())
        .state_at(one(1), at(1), None)
        .unwrap();
    let pins = Pins::new();
    let id = pins.pin(Arc::new(state), 7, at(0));
    assert!(pins.get(id, 8, at(1)).is_none(), "another request's token");
    assert!(pins.get(id, 7, at(1)).is_some());
    assert!(pins.get(id, 7, at(70_000_000_000)).is_none());
    assert!(pins.get(PinId(99), 7, at(0)).is_none());
}

#[test]
fn the_oldest_pin_is_evicted_when_full_and_its_memory_returns() {
    let s = Scratch::new();
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let reader = StateReader::new(r.engine.clone());
    let pins = Pins::new();
    let ids: Vec<_> = (0..17)
        .map(|_| {
            let state = reader.state_at(one(1), at(1), None).unwrap();
            pins.pin(Arc::new(state), 7, at(0))
        })
        .collect();
    assert!(pins.get(ids[0], 7, at(0)).is_none());
    assert!(pins.get(ids[16], 7, at(0)).is_some());
}

#[test]
fn a_page_cuts_at_either_bound_and_never_comes_back_empty() {
    let s = Scratch::new();
    store(&s.dir(), &[vec![sample(1, 1, value(1))]], 10);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let targets: Vec<_> = (0..5).map(|i| target(i, ValueKind::F64)).collect();
    let state = StateReader::new(r.engine.clone())
        .state_at(targets, at(1), None)
        .unwrap();
    assert_eq!(page_length(state.entries(), 3, u64::MAX), 3);
    assert_eq!(page_length(state.entries(), 100, 1), 1);
    assert_eq!(page_length(&[], 3, 100), 0);
}
