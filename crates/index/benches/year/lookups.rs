//! Lookup latency: each scenario starts from an empty index cache and a warm OS
//! page cache, with random signals and times from a fixed-seed generator.

use std::time::Instant;

use norn_core::{SignalId, Timestamp};
use norn_index::{PlanRequest, SignalFilter, TemporalIndex};

use super::gen_::{HOUR_NANOS, Layout, START_NANOS};

/// xorshift64*, so a run repeats exactly without a dependency.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D) % n.max(1)
    }
}

fn at(nanos: i64) -> Timestamp {
    Timestamp::from_nanos(nanos)
}

fn time(ns: u64) -> String {
    if ns >= 1_000_000 {
        format!("{:.2} ms", ns as f64 / 1e6)
    } else {
        format!("{:.1} µs", ns as f64 / 1e3)
    }
}

fn measure(index: &TemporalIndex, name: &str, runs: usize, mut f: impl FnMut() -> usize) {
    index.clear_cache();
    let before = index.stats();
    let mut times = Vec::with_capacity(runs);
    let mut chunks = 0;
    for _ in 0..runs {
        let t = Instant::now();
        chunks += f();
        times.push(t.elapsed().as_nanos() as u64);
    }
    times.sort_unstable();
    let after = index.stats();
    let hits = after.cache_hits - before.cache_hits;
    let misses = after.cache_misses - before.cache_misses;
    println!(
        "| {name} | {runs} | {} | {} | {} | {} | {:.0}% |",
        chunks / runs,
        time(times[times.len() / 2]),
        time(times[times.len() * 99 / 100]),
        time(times[times.len() - 1]),
        100.0 * hits as f64 / (hits + misses).max(1) as f64
    );
}

fn count(index: &TemporalIndex, request: PlanRequest) -> usize {
    let snapshot = index.snapshot();
    snapshot
        .plan(request)
        .map(|l| {
            l.unwrap();
        })
        .count()
}

pub fn all(index: &TemporalIndex, l: &Layout, samples: usize) {
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let hours = l.hours as i64;
    let month = hours.min(720);
    let label = format!("one signal, {month} hours");
    let range = |rng: &mut Rng, span: i64| {
        let start = START_NANOS + rng.below((hours - span + 1).max(1) as u64) as i64 * HOUR_NANOS;
        (at(start), at(start + span * HOUR_NANOS - 1))
    };
    measure(index, "one signal, one hour", samples, || {
        let (from, to) = range(&mut rng, 1);
        let signal = SignalId::new(rng.below(l.signals) * 2);
        count(
            index,
            PlanRequest::new(from, to).signals(SignalFilter::one(signal)),
        )
    });
    measure(index, &label, (samples / 5).max(1), || {
        let (from, to) = range(&mut rng, month);
        let signal = SignalId::new(rng.below(l.signals) * 2);
        count(
            index,
            PlanRequest::new(from, to).signals(SignalFilter::one(signal)),
        )
    });
    measure(
        index,
        "all signals, one minute",
        (samples / 5).max(1),
        || {
            let (from, _) = range(&mut rng, 1);
            let to = from.checked_add_nanos(59_999_999_999).unwrap();
            count(index, PlanRequest::new(from, to))
        },
    );
    measure(index, "nearest at or before", samples, || {
        let span = (hours * HOUR_NANOS) as u64;
        let t = at(START_NANOS + rng.below(span) as i64);
        let signal = SignalId::new(rng.below(l.signals) * 2);
        let snapshot = index.snapshot();
        usize::from(snapshot.nearest_at_or_before(signal, t).unwrap().is_some())
    });
    measure(
        index,
        "nearest, signal absent everywhere",
        20.min(samples),
        || {
            let signal = SignalId::new(rng.below(l.signals) * 2 + 1);
            let snapshot = index.snapshot();
            let found = snapshot.nearest_at_or_before(signal, Timestamp::MAX);
            usize::from(found.unwrap().is_some())
        },
    );
}
