//! Driving a replay session through the tool, and what becomes of sessions.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::seed::{TEMP, of, seed, time};
use common::{Running, stdio, temp};
use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::replay::{CreateReplaySessionRequest, ListReplaySessionsRequest, ReplaySession};
use serde_json::{Value, json};
use tonic::transport::Channel;

fn name(session: &Value) -> String {
    session["session"].as_str().unwrap().to_owned()
}

fn event_times(result: &Value) -> Vec<&str> {
    let events = result["events"].as_array().unwrap();
    events.iter().map(|e| e["time"].as_str().unwrap()).collect()
}

async fn open_sessions(channel: &Channel) -> usize {
    let list = ListReplaySessionsRequest::default();
    let reply = ReplayServiceClient::new(channel.clone())
        .list_replay_sessions(list)
        .await;
    reply.unwrap().into_inner().replay_sessions.len()
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_drives_a_session_and_sees_exactly_what_replay_order_gives() {
    let daemon = Running::start(temp("replay")).await;
    let channel = daemon.channel().await;
    seed(&channel).await;
    let c = stdio(&daemon.endpoint).await;
    let start = json!({"action": "start", "target": TEMP, "from": "1000", "to": "1010"});
    let session = c.ok("replay", start).await;
    assert_eq!(session["state"], "PAUSED");
    assert_eq!(session["clock"], time(1000));
    let id = name(&session);

    let args = json!({"action": "step", "session": id, "events": 3});
    let stepped = c.ok("replay", args).await;
    assert_eq!(event_times(&stepped), [time(1000), time(1001), time(1002)]);
    assert_eq!(stepped["samples_emitted"], 3);
    let want = of(TEMP, |r| r.second == 1001);
    assert_eq!(stepped["events"][1]["value"], want[0].value);

    let args = json!({"action": "seek", "session": id, "time": "1005"});
    let sought = c.ok("replay", args).await;
    assert_eq!(sought["clock"], time(1005));
    assert_eq!(event_times(&sought), [time(1004)]);
    assert_eq!(sought["events"][0]["event"], "state");

    let args = json!({"action": "step", "session": id, "duration": "2s"});
    assert_eq!(
        event_times(&c.ok("replay", args).await),
        [time(1005), time(1006)]
    );

    let args = json!({"action": "step", "session": id, "duration": "1h", "limit": 2});
    let refused = c.fails("replay", args).await;
    assert_eq!(refused["reason"], "MCP_LIMIT_EXCEEDED");
    let args = json!({"action": "status", "session": id});
    assert_eq!(
        c.ok("replay", args.clone()).await["clock"],
        time(1007),
        "nothing moved"
    );

    let args = json!({"action": "step", "session": id, "events": 100});
    let last = c.ok("replay", args).await;
    assert_eq!(last["count"], 3);
    assert_eq!(last["ended"], true);
    assert_eq!(last["state"], "ENDED");
    let args = json!({"action": "step", "session": id, "events": 1});
    let conflict = c.fails("replay", args).await;
    assert_eq!(conflict["reason"], "ERROR_REASON_REPLAY_STATE_CONFLICT");
    assert!(conflict["hint"].as_str().unwrap().contains("seek"));

    assert_eq!(
        c.ok("replay", json!({"action": "list"})).await["sessions"][0],
        id.as_str()
    );
    c.ok("replay", json!({"action": "stop", "session": id}))
        .await;
    assert_eq!(open_sessions(&channel).await, 0);
    let gone = c
        .fails("replay", json!({"action": "status", "session": id}))
        .await;
    assert_eq!(gone["reason"], "MCP_UNKNOWN_SESSION");
    c.service.cancel().await.unwrap();
    daemon.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn sessions_are_bounded_and_another_clients_session_is_out_of_reach() {
    let daemon = Running::start(temp("own")).await;
    let channel = daemon.channel().await;
    seed(&channel).await;
    let theirs = ReplaySession {
        interval: Some(Interval {
            start_time: Some(prost_types::Timestamp {
                seconds: 1_000,
                nanos: 0,
            }),
            end_time: Some(prost_types::Timestamp {
                seconds: 1_001,
                nanos: 0,
            }),
        }),
        scope: "entities/ahu".to_owned(),
        ..ReplaySession::default()
    };
    let request = CreateReplaySessionRequest {
        replay_session_id: "theirs".to_owned(),
        replay_session: Some(theirs),
    };
    ReplayServiceClient::new(channel.clone())
        .create_replay_session(request)
        .await
        .unwrap();

    let c = stdio(&daemon.endpoint).await;
    let args = json!({"action": "stop", "session": "replaySessions/theirs"});
    assert_eq!(
        c.fails("replay", args).await["reason"],
        "MCP_UNKNOWN_SESSION"
    );
    let start = json!({"action": "start", "target": TEMP, "from": "1000", "to": "1010"});
    for _ in 0..8 {
        c.ok("replay", start.clone()).await;
    }
    let refused = c.fails("replay", start).await;
    assert_eq!(refused["reason"], "MCP_LIMIT_EXCEEDED");
    assert_eq!(open_sessions(&channel).await, 9);
    c.service.cancel().await.unwrap();
    daemon.finish().await;
}
