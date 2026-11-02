//! One signal with a sample a second, for the control tests.
#![allow(dead_code, clippy::unwrap_used)]

use norn_core::Value;
use norn_query::QueryLimits;

use super::common::{Rig, Scratch, rig, sample, store};
use super::drive::{Seen, samples};

pub const SECOND: i64 = 1_000_000_000;

/// One signal, a sample every second from 0 s to 9 s.
pub fn ten() -> (Scratch, Rig) {
    let s = Scratch::new();
    let list = (0..10)
        .map(|i| sample(0, i * SECOND, Value::F64(i as f64)))
        .collect();
    store(&s.dir(), &[list], 4);
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    (s, r)
}

pub fn times(seen: &[Seen]) -> Vec<i64> {
    samples(seen).iter().map(|s| s.1 / SECOND).collect()
}
