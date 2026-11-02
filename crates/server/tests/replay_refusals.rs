//! Replay's refusals, each by its reason, over real gRPC.
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

use common::{seed, start, temp};
use events_common::{create, replays};
use norn_core::{Config, Limits};
use norn_protocol::common::ErrorReason;
use norn_protocol::common::ValueType::Double;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::step_replay_session_request::Step;
use norn_protocol::replay::{
    ListReplaySessionsRequest, PauseReplaySessionRequest, ResumeReplaySessionRequest,
    StepReplaySessionRequest,
};
use norn_server::DaemonConfig;
use query_common::reason;
use tonic::Code;

#[tokio::test(flavor = "multi_thread")]
async fn refusals_are_typed_by_reason() {
    let mut limits = Limits::default();
    limits.max_replay_sessions = 2;
    let engine = Config::builder().limits(limits).build().unwrap();
    let d = start(DaemonConfig::new(engine), temp("replay-refuse")).await;
    seed(&d.channel, &[("a", Double)]).await;
    let mut client = replays(&d.channel);
    let one = create(&d.channel, 1_000, 1_010, Pace::Speed(1.0)).await;
    create(&d.channel, 1_000, 1_010, Pace::Speed(1.0)).await;
    let third = client.create_replay_session(norn_protocol::replay::CreateReplaySessionRequest {
        replay_session: Some(one.clone()),
        ..Default::default()
    });
    let refused = third.await.unwrap_err();
    assert_eq!(
        (refused.code(), reason(&refused)),
        (
            Code::ResourceExhausted,
            ErrorReason::CapacityExhausted.as_str_name().into()
        )
    );

    let stale = PauseReplaySessionRequest {
        name: one.name.clone(),
        etag: "\"g99\"".into(),
    };
    let refused = client.pause_replay_session(stale).await.unwrap_err();
    assert_eq!(refused.code(), Code::Aborted);
    let step = StepReplaySessionRequest {
        name: one.name.clone(),
        step: None,
        etag: String::new(),
    };
    assert_eq!(
        client.step_replay_session(step).await.unwrap_err().code(),
        Code::InvalidArgument
    );
    let resume = ResumeReplaySessionRequest {
        name: one.name.clone(),
        etag: one.etag.clone(),
    };
    client.resume_replay_session(resume).await.unwrap();
    let step = StepReplaySessionRequest {
        name: one.name.clone(),
        step: Some(Step::EventCount(1)),
        etag: String::new(),
    };
    let refused = client.step_replay_session(step).await.unwrap_err();
    assert_eq!(
        reason(&refused),
        ErrorReason::ReplayStateConflict.as_str_name()
    );
    let missing = PauseReplaySessionRequest {
        name: "replaySessions/nope".into(),
        etag: String::new(),
    };
    assert_eq!(
        client
            .pause_replay_session(missing)
            .await
            .unwrap_err()
            .code(),
        Code::NotFound
    );

    let page = ListReplaySessionsRequest {
        page_size: 1,
        page_token: String::new(),
    };
    let first = client
        .list_replay_sessions(page)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(first.replay_sessions[0].name, one.name);
    let page = ListReplaySessionsRequest {
        page_size: 1,
        page_token: first.next_page_token,
    };
    let second = client
        .list_replay_sessions(page)
        .await
        .unwrap()
        .into_inner();
    assert_eq!(second.replay_sessions.len(), 1);
    assert!(second.next_page_token.is_empty());
    let _ = std::fs::remove_dir_all(d.stop().await);
}
