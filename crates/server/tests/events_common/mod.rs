//! Replay sessions and subscriptions the way a client uses them.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_protocol::common::Sample;
use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{CreateReplaySessionRequest, ReplaySession};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{
    ClockEventKind, Event, SubscribeEventsRequest, SubscribeEventsResponse, SubscriptionMode, event,
};
use tonic::Streaming;
use tonic::transport::Channel;

pub fn seconds(s: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp {
        seconds: s,
        nanos: 0,
    })
}

pub fn replays(c: &Channel) -> ReplayServiceClient<Channel> {
    ReplayServiceClient::new(c.clone())
}

/// A session over `entities/ahu`, from `start` to `end` seconds.
pub async fn create(c: &Channel, start: i64, end: i64, pace: Pace) -> ReplaySession {
    let session = ReplaySession {
        interval: Some(Interval {
            start_time: seconds(start),
            end_time: seconds(end),
        }),
        scope: "entities/ahu".into(),
        pace: Some(pace),
        ..ReplaySession::default()
    };
    let request = CreateReplaySessionRequest {
        replay_session: Some(session),
        ..CreateReplaySessionRequest::default()
    };
    replays(c)
        .create_replay_session(request)
        .await
        .unwrap()
        .into_inner()
}

pub async fn subscribe(
    c: &Channel,
    request: SubscribeEventsRequest,
) -> Result<Streaming<SubscribeEventsResponse>, tonic::Status> {
    let mut client = TelemetryServiceClient::new(c.clone());
    Ok(client.subscribe_events(request).await?.into_inner())
}

pub fn replay(name: &str) -> SubscribeEventsRequest {
    SubscribeEventsRequest {
        mode: SubscriptionMode::Replay as i32,
        replay_session: name.to_owned(),
        ..SubscribeEventsRequest::default()
    }
}

pub fn live() -> SubscribeEventsRequest {
    SubscribeEventsRequest {
        mode: SubscriptionMode::Live as i32,
        parent: "entities/ahu".into(),
        ..SubscribeEventsRequest::default()
    }
}

pub fn kind(e: &Event) -> Option<ClockEventKind> {
    match &e.payload {
        Some(event::Payload::ClockEvent(c)) => ClockEventKind::try_from(c.kind).ok(),
        _ => None,
    }
}

pub fn sample(e: &Event) -> Option<&Sample> {
    match &e.payload {
        Some(event::Payload::Sample(s)) => Some(s),
        _ => None,
    }
}

/// Events after number `after` until one satisfies `last`, which is included,
/// checking that the numbering goes up by exactly one.
pub async fn until(
    s: &mut Streaming<SubscribeEventsResponse>,
    after: i64,
    last: impl Fn(&Event) -> bool,
) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    loop {
        let response = s.message().await.unwrap().expect("the stream ended early");
        assert!(!response.events.is_empty(), "an empty response");
        for e in response.events {
            let want = out.last().map_or(after + 1, |p| p.event_sequence + 1);
            assert_eq!(e.event_sequence, want, "a gap or a repeat");
            let done = last(&e);
            out.push(e);
            if done {
                return out;
            }
        }
    }
}
