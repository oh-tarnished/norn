//! LIVE subscriptions over real gRPC.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod events_common;
#[path = "query_common/mod.rs"]
mod query_common;

use std::time::{Duration, Instant};

use common::{append, batch, double, seed, start, temp};
use events_common::{live, sample, subscribe, until};
use norn_protocol::common::ErrorReason;
use norn_protocol::common::ValueType::Double;
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_protocol::telemetry::{SubscribeEventsRequest, SubscriptionMode};
use norn_server::DaemonConfig;
use query_common::reason;
use tonic::Code;

#[tokio::test(flavor = "multi_thread")]
async fn a_live_subscriber_gets_every_sample_appended_after_it_numbered_without_gaps() {
    let d = start(DaemonConfig::default(), temp("live-all")).await;
    seed(&d.channel, &[("a", Double), ("b", Double)]).await;
    append(&d.channel, vec![batch(1, vec![double("a", 0)], Persisted)]).await;
    let mut only_a = SubscribeEventsRequest {
        filter: "id = \"a\"".into(),
        ..live()
    };
    only_a.mode = SubscriptionMode::Live as i32;
    let mut a = subscribe(&d.channel, only_a).await.unwrap();
    let mut both = subscribe(&d.channel, live()).await.unwrap();
    for n in 1..=50 {
        append(
            &d.channel,
            vec![batch(1, vec![double("a", n), double("b", n)], Persisted)],
        )
        .await;
    }
    let got = until(&mut both, 0, |e| e.event_sequence == 100).await;
    let mut seqs: Vec<_> = got.iter().map(|e| sample(e).unwrap().sequence).collect();
    seqs.dedup();
    assert_eq!(
        seqs,
        (1..=50).collect::<Vec<_>>(),
        "nothing from before it started"
    );
    assert!(
        got.iter()
            .all(|e| e.mode == SubscriptionMode::Live as i32 && e.clock_time.is_some())
    );
    let got = until(&mut a, 0, |e| e.event_sequence == 50).await;
    assert!(
        got.iter()
            .all(|e| sample(e).unwrap().signal.ends_with("/a"))
    );
    let _ = std::fs::remove_dir_all(d.stop().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_slow_live_subscriber_is_disconnected_and_ingestion_does_not_wait_for_it() {
    let d = start(DaemonConfig::default(), temp("live-slow")).await;
    seed(&d.channel, &[("a", Double)]).await;
    let mut slow = subscribe(&d.channel, live()).await.unwrap();
    let mut fast = subscribe(&d.channel, live()).await.unwrap();
    let reader = tokio::spawn(async move {
        until(&mut fast, 0, |e| e.event_sequence == 40_000)
            .await
            .len()
    });
    let began = Instant::now();
    for n in 0..400 {
        let list = (n * 100..(n + 1) * 100).map(|i| double("a", i)).collect();
        let replies = append(&d.channel, vec![batch(1, list, Persisted)]).await;
        assert_eq!(
            replies.last().unwrap().ack_level,
            Persisted as i32,
            "batch {n}"
        );
    }
    assert!(
        began.elapsed() < Duration::from_secs(60),
        "ingestion waited"
    );
    assert_eq!(
        reader.await.unwrap(),
        40_000,
        "a reading subscriber missed nothing"
    );
    let mut seen = 0;
    let ended = loop {
        match slow.message().await {
            Ok(Some(r)) => {
                for e in r.events {
                    seen += 1;
                    assert_eq!(e.event_sequence, seen, "a gap before the end");
                }
            }
            Ok(None) => panic!("closed without saying why"),
            Err(status) => break status,
        }
    };
    assert!(seen < 40_000, "the slow one kept up after all");
    assert_eq!(ended.code(), Code::ResourceExhausted);
    assert_eq!(reason(&ended), ErrorReason::SubscriberTooSlow.as_str_name());
    let _ = std::fs::remove_dir_all(d.stop().await);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_subscription_with_fields_of_the_other_mode_is_refused() {
    let d = start(DaemonConfig::default(), temp("live-bad")).await;
    seed(&d.channel, &[("a", Double)]).await;
    let mixed = SubscribeEventsRequest {
        replay_session: "replaySessions/x".into(),
        ..live()
    };
    assert_eq!(
        subscribe(&d.channel, mixed).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    let none = SubscribeEventsRequest::default();
    assert_eq!(
        subscribe(&d.channel, none).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    let missing = events_common::replay("replaySessions/nope");
    assert_eq!(
        subscribe(&d.channel, missing).await.unwrap_err().code(),
        Code::NotFound
    );
    let _ = std::fs::remove_dir_all(d.stop().await);
}
