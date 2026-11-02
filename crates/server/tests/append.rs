//! `AppendSamples` over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{append, batch, double, seed, start, stored, temp};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Value, ValueType};
use norn_protocol::rpc::pb;
use norn_protocol::telemetry::AcknowledgementLevel::{Buffered, Durable, Persisted, Unspecified};
use norn_protocol::telemetry::{BackpressureOutcome, BackpressureReason};
use norn_server::DaemonConfig;

const SIGNALS: [(&str, ValueType); 2] = [("t", ValueType::Double), ("n", ValueType::Uint32)];

fn outcome(r: &norn_protocol::telemetry::AppendSamplesResponse) -> (i32, i32) {
    let a = r.admission.as_ref().unwrap();
    (a.outcome, a.reason)
}

fn reason_of(status: &pb::Status) -> String {
    use prost::Message;
    let any = status
        .details
        .iter()
        .find(|d| d.type_url.ends_with("ErrorInfo"))
        .unwrap();
    pb::ErrorInfo::decode(&any.value[..]).unwrap().reason
}

#[tokio::test(flavor = "multi_thread")]
async fn a_durable_batch_gets_exactly_two_responses_and_a_buffered_one_gets_one() {
    let d = start(DaemonConfig::default(), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let replies = append(
        &d.channel,
        vec![
            batch(1, vec![double("t", 1)], Durable),
            batch(2, vec![double("t", 2)], Buffered),
            batch(3, vec![double("t", 3)], Persisted),
        ],
    )
    .await;
    let of = |id| replies.iter().filter(|r| r.batch_id == id).count();
    assert_eq!((of(1), of(2), of(3)), (2, 1, 2), "{replies:?}");
    assert_eq!(replies[0].batch_id, 1);
    assert_eq!(replies[0].ack_level, Buffered as i32);
    let last = replies.iter().rfind(|r| r.batch_id == 1).unwrap();
    assert_eq!(last.ack_level, Durable as i32);
    assert_eq!(outcome(last).0, BackpressureOutcome::Accept as i32);
    let data = d.stop().await;
    assert_eq!(stored(&data).len(), 3);
}

#[tokio::test(flavor = "multi_thread")]
async fn one_bad_sample_refuses_the_whole_batch_and_every_bad_sample_is_listed() {
    let d = start(DaemonConfig::default(), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let mut wrong_type = double("n", 2);
    wrong_type.value = Some(Value {
        kind: Some(Kind::DoubleValue(1.0)),
    });
    let mut out_of_range = double("n", 3);
    out_of_range.value = Some(Value {
        kind: Some(Kind::Uint32Value(1 << 32)),
    });
    let mut negative = double("t", 4);
    negative.sequence = -1;
    let samples = vec![
        double("t", 1),
        double("ghost", 1),
        wrong_type,
        out_of_range,
        negative,
    ];
    let replies = append(&d.channel, vec![batch(1, samples, Durable)]).await;
    assert_eq!(replies.len(), 1);
    let r = &replies[0];
    let reject = BackpressureOutcome::Reject as i32;
    assert_eq!(
        outcome(r),
        (reject, BackpressureReason::InvalidSamples as i32)
    );
    assert_eq!(r.ack_level, Unspecified as i32);
    let indexes: Vec<_> = r.sample_errors.iter().map(|e| e.index).collect();
    assert_eq!(indexes, [1, 2, 3, 4]);
    let reasons: Vec<_> = r
        .sample_errors
        .iter()
        .map(|e| reason_of(e.status.as_ref().unwrap()))
        .collect();
    assert_eq!(reasons[0], "ERROR_REASON_SIGNAL_NOT_FOUND");
    assert_eq!(reasons[1], "ERROR_REASON_VALUE_TYPE_MISMATCH");
    assert_eq!(reasons[2], "ERROR_REASON_INVALID_ARGUMENT");
    let data = d.stop().await;
    assert!(
        stored(&data).is_empty(),
        "nothing of a refused batch is stored"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_retried_batch_is_counted_as_duplicates_and_stored_once() {
    let d = start(DaemonConfig::default(), temp("append")).await;
    seed(&d.channel, &SIGNALS).await;
    let samples = || (1..=50).map(|n| double("t", n)).collect::<Vec<_>>();
    append(&d.channel, vec![batch(1, samples(), Durable)]).await;
    let again = append(&d.channel, vec![batch(1, samples(), Durable)]).await;
    assert_eq!(again[0].duplicate_count, 50);
    assert_eq!(again.last().unwrap().ack_level, Durable as i32);
    let data = d.stop().await;
    assert_eq!(stored(&data).len(), 50);
}
