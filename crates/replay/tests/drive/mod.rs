//! Driving a session the way the runtime does, on a clock the test moves.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use norn_core::{Sample, Timestamp, ValueKind};
use norn_query::Engine;
use norn_replay::{ClockKind, Next, Pace, Payload, Session, SessionSpec};

use super::common::{at, target};

/// What a subscriber would see, without numbering.
#[derive(Debug, Clone, PartialEq)]
pub enum Seen {
    Sample(String, Sample),
    State(String, Sample),
    Clock(ClockKind, i64),
}

pub fn session(engine: &Arc<Engine>, signals: u64, start: i64, end: i64, pace: Pace) -> Session {
    let spec = SessionSpec {
        start: at(start),
        end: at(end),
        targets: (0..signals).map(|s| target(s, ValueKind::F64)).collect(),
        pace,
    };
    Session::new(spec, Arc::clone(engine))
}

/// A wall clock in nanoseconds, moved only by `run`.
pub struct Wall(pub i64);

impl Wall {
    pub fn now(&self) -> Timestamp {
        at(self.0)
    }
}

pub fn seen(batches: impl IntoIterator<Item = norn_replay::Batch>) -> Vec<Seen> {
    let mut out = Vec::new();
    for batch in batches {
        for e in batch.events {
            out.push(match e.payload {
                Payload::Sample { signal, sample } => Seen::Sample(signal.to_string(), sample),
                Payload::State { signal, sample } => Seen::State(signal.to_string(), sample),
                Payload::Clock(kind) => Seen::Clock(kind, e.clock_time.as_nanos()),
                _ => panic!("an unknown payload"),
            });
        }
    }
    out
}

/// Polls until the session goes idle, sleeping through every wait (each at most
/// `max_sleep`, as a subscriber's pace might cut it), and returns what it emitted.
pub fn run(s: &mut Session, wall: &mut Wall, max_sleep: Duration) -> Vec<Seen> {
    let mut out = Vec::new();
    for _ in 0..1_000_000 {
        match s.poll(wall.now()) {
            Next::Emit(batch) => out.extend(seen([batch])),
            Next::Read => s.fill(wall.now()).unwrap(),
            Next::Wait(d) => wall.0 += d.min(max_sleep).as_nanos() as i64,
            _ => return out,
        }
    }
    panic!("the session never went idle");
}

/// Only the samples, as (name, time, sequence).
pub fn samples(seen: &[Seen]) -> Vec<(String, i64, u64)> {
    let keep = |s: &Seen| match s {
        Seen::Sample(n, x) => Some((n.clone(), x.timestamp.as_nanos(), x.sequence.get())),
        _ => None,
    };
    seen.iter().filter_map(keep).collect()
}
