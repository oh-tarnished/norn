//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]

//! A whole outage as a table: a fake clock and made-up answers.

mod gate;
mod recovery;
mod share;

use super::{Flow, Outcome};
use crate::health::Notice;
use norn_protocol::common::Sample;
use norn_protocol::telemetry::{
    AcknowledgementLevel as Ack, Admission, AppendSamplesResponse, BackpressureOutcome as O,
    BackpressureReason as R, SampleError,
};
use std::time::Duration;

pub(super) const T0: Duration = Duration::from_secs(1);

pub(super) fn samples(n: usize) -> Vec<Sample> {
    (0..n)
        .map(|i| Sample {
            signal: format!("entities/e/signals/s{i}"),
            sequence: i64::try_from(i).unwrap(),
            ..Sample::default()
        })
        .collect()
}

/// A flow whose byte bound holds exactly `samples(capacity)`, so a test
/// can count in samples while the bound is in bytes.
pub(super) fn flow(capacity: usize) -> Flow {
    let bytes = samples(capacity).iter().map(super::hold::cost).sum();
    Flow::new("streams/s".into(), Ack::Persisted, bytes)
}

pub(super) fn first(id: i64, outcome: O, reason: R, delay: Option<i64>) -> AppendSamplesResponse {
    AppendSamplesResponse {
        batch_id: id,
        admission: Some(Admission {
            outcome: outcome as i32,
            reason: reason as i32,
            retry_delay: delay.map(|seconds| prost_types::Duration { seconds, nanos: 0 }),
            message: "m".into(),
        }),
        ack_level: Ack::Buffered as i32,
        ..AppendSamplesResponse::default()
    }
}

pub(super) fn second(id: i64) -> AppendSamplesResponse {
    AppendSamplesResponse {
        ack_level: Ack::Persisted as i32,
        ..first(id, O::Accept, R::CapacityAvailable, None)
    }
}

#[test]
fn a_batch_is_held_until_it_reaches_the_level_asked_for() {
    let mut f = flow(10);
    f.offer(samples(3));
    let batch = f.take(T0).unwrap();
    assert_eq!((batch.batch_id, batch.samples.len()), (1, 3));
    f.answer(&first(1, O::Accept, R::CapacityAvailable, None), T0);
    assert_eq!((f.held(), f.counts().acknowledged), (3, 0));
    f.answer(&second(1), T0);
    assert_eq!((f.held(), f.counts().acknowledged), (0, 3));
}

#[test]
fn buffer_admits_the_batch_and_changes_nothing_about_what_goes_next() {
    let mut f = flow(10);
    f.offer(samples(1));
    f.take(T0).unwrap();
    f.answer(&first(1, O::Buffer, R::WritePathBehind, None), T0);
    f.offer(samples(1));
    assert!(f.ready(T0));
}

#[test]
fn throttle_admits_the_batch_and_holds_the_next_for_exactly_the_delay() {
    let mut f = flow(10);
    f.offer(samples(1));
    f.take(T0).unwrap();
    f.answer(&first(1, O::Throttle, R::BufferHighWater, Some(2)), T0);
    f.answer(&second(1), T0);
    assert_eq!(f.counts().acknowledged, 1, "throttle stored this batch");
    f.offer(samples(1));
    let later = T0 + Duration::from_secs(2);
    assert_eq!(f.wake(T0), Some(later));
    assert!(f.take(T0).is_none());
    assert!(
        f.take(later.saturating_sub(Duration::from_nanos(1)))
            .is_none()
    );
    assert!(f.take(later).is_some());
}

#[test]
fn a_transient_reject_resends_the_same_batch_after_its_delay() {
    let mut f = flow(10);
    f.offer(samples(2));
    let sent = f.take(T0).unwrap();
    let reply = first(1, O::Reject, R::BufferFull, Some(3));
    assert_eq!(f.answer(&reply, T0), Outcome::Fine);
    assert!(f.take(T0 + Duration::from_secs(2)).is_none());
    let again = f.take(T0 + Duration::from_secs(3)).unwrap();
    assert_eq!(again.samples, sent.samples);
    assert_eq!(f.counts().rejected, 1);
}

#[test]
fn a_permanent_reject_is_told_held_and_backed_off() {
    let mut f = flow(10);
    f.offer(samples(2));
    f.take(T0).unwrap();
    let reply = first(1, O::Reject, R::StorageUnavailable, None);
    assert!(matches!(
        f.answer(&reply, T0),
        Outcome::Tell(Notice::Refused { .. })
    ));
    assert_eq!(f.held(), 2);
    assert!(f.take(T0).is_none());
    assert!(f.take(T0 + Duration::from_millis(250)).is_some());
}

#[test]
fn invalid_samples_are_counted_out_and_the_rest_go_on() {
    let mut f = flow(10);
    f.offer(samples(3));
    f.take(T0).unwrap();
    let mut reply = first(1, O::Reject, R::InvalidSamples, None);
    reply.sample_errors = vec![SampleError {
        index: 1,
        status: None,
    }];
    assert!(matches!(
        f.answer(&reply, T0),
        Outcome::Tell(Notice::Invalid { readings: 1, .. })
    ));
    assert_eq!((f.held(), f.counts().invalid), (2, 1));
    let batch = f.take(T0).unwrap();
    assert_eq!(batch.samples[0].sequence, 0);
    assert_eq!(batch.samples[1].sequence, 2);
}

#[test]
fn a_batch_too_large_comes_back_in_halves() {
    let mut f = flow(100);
    f.offer(samples(8));
    f.take(T0).unwrap();
    f.answer(&first(1, O::Reject, R::BatchTooLarge, None), T0);
    assert_eq!(f.take(T0).unwrap().samples.len(), 4);
}

#[test]
fn a_full_buffer_gives_up_the_oldest_and_counts_them() {
    let mut f = flow(3);
    f.offer(samples(2));
    assert_eq!(f.offer(samples(3)), 2);
    assert_eq!((f.held(), f.counts().overflowed), (3, 2));
    assert_eq!(f.take(T0).unwrap().samples[0].sequence, 0);
}
