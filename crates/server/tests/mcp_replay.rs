//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Driving a replay through the tools, over stdio and a Unix socket, against a real
//! daemon: steps, a seek's state, a step too big refused before anything moves.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "mcp_common/mod.rs"]
mod mcp_common;
#[path = "reads_common/mod.rs"]
mod reads_common;

use mcp_common::{Client, Norn};
use reads_common::seed;
use serde_json::{Value, json};

fn time(second: i64) -> String {
    format!("1970-01-01T00:{:02}:{:02}Z", second / 60, second % 60)
}

fn interval(from: i64, to: i64) -> Value {
    json!({"startTime": time(from), "endTime": time(to)})
}

async fn replay(c: &Client) {
    let session = json!({"interval": interval(1000, 1010), "scope": "entities/dev",
        "filter": "id = \"temp\"", "speed": 0.0});
    let args = json!({"replay_session": session});
    let made = c.ok("replay_service-create_replay_session_v1", args).await;
    let name = made["name"].as_str().unwrap().to_owned();
    let drive = "replay_service-drive_replay_session_v1";
    let got = c.ok(drive, json!({"name": name, "event_count": 3})).await;
    let at = |e: &Value| e["sample"]["observeTime"].as_str().unwrap().to_owned();
    let events = got["replayEvents"].as_array().unwrap();
    assert_eq!(
        events.iter().map(at).collect::<Vec<_>>(),
        [time(1000), time(1001), time(1002)]
    );
    let sought = c
        .ok(drive, json!({"name": name, "seek_time": time(1005)}))
        .await;
    assert_eq!(
        sought["replayEvents"][0]["stateSample"]["observeTime"],
        time(1004)
    );
    let huge = c
        .fails(
            drive,
            json!({"name": name, "duration": "3600s", "max_events": 2}),
        )
        .await;
    assert_eq!(huge["reason"], "ERROR_REASON_LIMIT_EXCEEDED");
    let one = c
        .ok(
            "replay_service-get_replay_session_v1",
            json!({"name": name}),
        )
        .await;
    assert_eq!(one["currentTime"], time(1005), "nothing moved");
    let listed = c
        .ok("replay_service-list_replay_sessions_v1", json!({}))
        .await;
    assert_eq!(listed["replaySessions"].as_array().unwrap().len(), 1);
    c.ok(
        "replay_service-delete_replay_session_v1",
        json!({"name": name}),
    )
    .await;
    let gone = c
        .fails(
            "replay_service-get_replay_session_v1",
            json!({"name": name}),
        )
        .await;
    assert_eq!(gone["reason"], "MCP_UNKNOWN_SESSION");
}

#[tokio::test(flavor = "multi_thread")]
async fn an_agent_drives_a_replay_over_stdio_and_a_unix_socket() {
    let mut norn = Norn::both("replay").await;
    seed(&norn.channel).await;
    for client in [norn.unix().await, norn.stdio().await] {
        replay(&client).await;
    }
    norn.finish().await;
}
