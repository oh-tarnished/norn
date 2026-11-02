//! Live fan-out: never waits, never skips, ends a reader that falls behind.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::sync::Arc;
use std::time::{Duration, Instant};

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value};
use norn_runtime::live::{LIVE_QUEUE_BATCHES, Live};
use norn_runtime::{Budget, Clock, End, FakeClock};

fn batch(n: u64) -> Vec<Sample> {
    let s = Sample::new(
        Timestamp::from_nanos(n as i64),
        SignalId::new(1),
        Value::F64(n as f64),
        Quality::GOOD,
        Sequence::new(n),
    );
    vec![s]
}

#[tokio::test]
async fn a_reader_that_falls_behind_is_ended_and_the_others_get_everything() {
    let live = Live::new(8);
    let mut slow = live.subscribe().unwrap();
    let mut fast = live.subscribe().unwrap();
    let began = Instant::now();
    let mut seen = Vec::new();
    for n in 0..10_000 {
        live.publish(StreamId::new(1), batch(n));
        while let Some(b) = fast.try_next() {
            seen.push(b.samples[0].sequence.get());
        }
    }
    assert!(
        began.elapsed() < Duration::from_secs(2),
        "publishing waited"
    );
    assert_eq!(seen, (0..10_000).collect::<Vec<_>>());
    for n in 0..LIVE_QUEUE_BATCHES as u64 {
        assert_eq!(slow.next().await.unwrap().samples[0].sequence.get(), n);
    }
    assert!(matches!(slow.next().await, Err(End::TooSlow)));
}

#[tokio::test]
async fn subscribers_are_limited_and_closing_ends_them() {
    let live = Live::new(1);
    let mut one = live.subscribe().unwrap();
    assert!(live.subscribe().is_none());
    drop(live.subscribe());
    live.close();
    assert!(matches!(one.next().await, Err(End::ShuttingDown)));
    assert!(live.subscribe().is_none());
}

#[tokio::test]
async fn a_full_budget_ends_the_readers_holding_it_and_spares_the_idle() {
    let live = Live::new(4);
    let clock: Arc<dyn Clock> = Arc::new(FakeClock::new(Timestamp::from_nanos(0)));
    live.charge_to(Budget::new("replay", 1_024, clock));
    let mut idle = live.subscribe().unwrap();
    let mut stuck = live.subscribe().unwrap();
    for n in 0..64 {
        live.publish(StreamId::new(1), batch(n));
        if let Some(b) = idle.try_next() {
            assert_eq!(b.samples[0].sequence.get(), n);
        }
    }
    let mut count = 0;
    while stuck.next().await.is_ok() {
        count += 1;
    }
    assert!(count < 64, "the budget bound nothing");
}
