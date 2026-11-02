//! Random data, laid out at random, and the brute-force answers.

use std::collections::BTreeMap;

use super::{Rig, Scratch, Seen, at, rig, store, target};
use norn_core::{Quality, Sample, Sequence, Severity, SignalId, Substatus, Value, ValueKind};
use norn_query::{Filter, Output, QueryLimits, QuerySpec, TimeRange};
use proptest::prelude::*;

pub const SIGNALS: u64 = 3;

pub fn quality(n: u8) -> Quality {
    match n % 4 {
        0 | 1 => Quality::GOOD,
        2 => Quality::new(Severity::Uncertain, Substatus::Stale),
        _ => Quality::new(Severity::Bad, Substatus::SensorFailure),
    }
}

/// Signal, time, sequence offset, value, quality: times collide on purpose.
pub type Raw = (u64, i64, u64, i32, u8);

pub fn raw() -> impl Strategy<Value = Vec<Raw>> {
    prop::collection::vec(
        (0..SIGNALS, 0i64..400, 0u64..3, -50i32..50, any::<u8>()),
        0..300,
    )
}

/// Distinct by (signal, time, sequence), as a real write path keeps them.
pub fn samples(raw: &[Raw]) -> Vec<Sample> {
    let mut by = BTreeMap::new();
    for (signal, t, seq, v, q) in raw {
        let value = Value::F64(f64::from(*v) / 4.0);
        let s = Sample::new(
            at(*t),
            SignalId::new(*signal),
            value,
            quality(*q),
            Sequence::new(*seq),
        );
        by.entry((*signal, *t, *seq)).or_insert(s);
    }
    by.into_values().collect()
}

/// Splits `all` into segments and a tail, the tail overlapping the last segment,
/// which is the moment a generation is in both places.
pub fn lay_out(s: &Scratch, r: &Rig0, all: &[Sample]) -> Rig {
    let cut = r.cut.min(all.len());
    let (stored, tail) = all.split_at(cut);
    let segments: Vec<Vec<Sample>> = stored
        .chunks(r.segment.max(1))
        .map(<[Sample]>::to_vec)
        .collect();
    store(&s.dir(), &segments, r.chunk);
    let rigged = rig(&s.0, 1 << 30, r.limits);
    let overlap = stored.len().saturating_sub(r.overlap);
    let mut buffered: Vec<Sample> = stored[overlap..].to_vec();
    buffered.extend_from_slice(tail);
    *rigged.tail.0.lock().unwrap() = buffered;
    rigged
}

pub struct Rig0 {
    pub cut: usize,
    pub segment: usize,
    pub chunk: usize,
    pub overlap: usize,
    pub limits: QueryLimits,
}

pub fn layout() -> impl Strategy<Value = (usize, usize, usize, usize, u64)> {
    (
        0usize..300,
        1usize..80,
        1usize..30,
        0usize..20,
        256u64..20_000,
    )
}

pub fn rig0((cut, segment, chunk, overlap, slice): (usize, usize, usize, usize, u64)) -> Rig0 {
    let mut limits = QueryLimits::default();
    limits.slice_bytes = slice;
    limits.page_results = 7;
    Rig0 {
        cut,
        segment,
        chunk,
        overlap,
        limits,
    }
}

pub fn filters() -> impl Strategy<Value = (&'static str, fn(&Sample) -> bool)> {
    prop_oneof![
        Just(("", (|_| true) as fn(&Sample) -> bool)),
        Just((
            "value > 2.5",
            (|s| matches!(s.value, Value::F64(v) if v > 2.5)) as fn(&Sample) -> bool
        )),
        Just((
            "quality.severity = QUALITY_SEVERITY_GOOD",
            (|s| s.quality.severity() == Severity::Good) as fn(&Sample) -> bool
        )),
        Just((
            "quality.severity != QUALITY_SEVERITY_BAD AND value <= 0",
            (|s| s.quality.severity() != Severity::Bad
                && matches!(s.value, Value::F64(v) if v <= 0.0)) as fn(&Sample) -> bool
        )),
        Just((
            "quality.substatus = QUALITY_SUBSTATUS_STALE",
            (|s| s.quality.substatus() == Substatus::Stale) as fn(&Sample) -> bool
        )),
    ]
}

pub fn spec(range: TimeRange, text: &str, output: Output) -> QuerySpec {
    QuerySpec {
        targets: (0..SIGNALS).map(|s| target(s, ValueKind::F64)).collect(),
        range,
        filter: Filter::parse(text).unwrap(),
        output,
        resume: None,
    }
}

pub fn oracle(all: &[Sample], from: i64, to: i64, keep: fn(&Sample) -> bool) -> Vec<Sample> {
    let inside = |s: &&Sample| (from..to).contains(&s.timestamp.as_nanos());
    all.iter()
        .filter(inside)
        .filter(|s| keep(s))
        .cloned()
        .collect()
}

pub fn key_time(s: &Sample) -> (i64, u64, u64) {
    (s.timestamp.as_nanos(), s.sequence.get(), s.signal.get())
}

pub fn expected_row(s: &Sample) -> (String, i64, u64, f64) {
    let Value::F64(v) = s.value else { panic!() };
    (
        target(s.signal.get(), ValueKind::F64).name.to_string(),
        s.timestamp.as_nanos(),
        s.sequence.get(),
        v,
    )
}

pub fn got_row((n, t, q, bits): Seen) -> (String, i64, u64, f64) {
    let (Value::F64(v), _) = Value::decode(&bits).unwrap() else {
        panic!()
    };
    (n, t, q, v)
}

pub fn number(v: &Value) -> f64 {
    match v {
        Value::F64(v) => *v,
        Value::I64(v) => *v as f64,
        _ => panic!("{v:?}"),
    }
}
