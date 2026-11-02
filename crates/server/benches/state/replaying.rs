//! Replay throughput: a session over the whole set, at maximum speed.

use std::time::Instant;

use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{
    CreateReplaySessionRequest, ReplaySession, ResumeReplaySessionRequest,
};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{ClockEventKind, SubscribeEventsRequest, SubscriptionMode, event};
use tonic::transport::Channel;

use super::timing::at;

/// Replays `from..to` of every signal to one subscriber, and returns the samples
/// it received and the seconds from resuming to ENDED.
pub async fn replay(channel: &Channel, from: i64, to: i64) -> (u64, f64) {
    let mut replays = ReplayServiceClient::new(channel.clone());
    let session = ReplaySession {
        interval: Some(Interval {
            start_time: at(from),
            end_time: at(to),
        }),
        scope: "entities/-".into(),
        pace: Some(Pace::MaxSpeed(true)),
        ..ReplaySession::default()
    };
    let create = CreateReplaySessionRequest {
        replay_session: Some(session),
        ..Default::default()
    };
    let name = replays
        .create_replay_session(create)
        .await
        .unwrap()
        .into_inner()
        .name;
    let request = SubscribeEventsRequest {
        mode: SubscriptionMode::Replay as i32,
        replay_session: name.clone(),
        ..SubscribeEventsRequest::default()
    };
    let mut client = TelemetryServiceClient::new(channel.clone());
    let mut events = client.subscribe_events(request).await.unwrap().into_inner();
    let began = Instant::now();
    let resume = ResumeReplaySessionRequest {
        name,
        etag: String::new(),
    };
    replays.resume_replay_session(resume).await.unwrap();
    let mut samples = 0;
    while let Some(page) = events.message().await.unwrap() {
        for e in page.events {
            match e.payload {
                Some(event::Payload::Sample(_)) => samples += 1,
                Some(event::Payload::ClockEvent(c)) if c.kind == ClockEventKind::Ended as i32 => {
                    return (samples, began.elapsed().as_secs_f64());
                }
                _ => {}
            }
        }
    }
    panic!("the replay never ended");
}
