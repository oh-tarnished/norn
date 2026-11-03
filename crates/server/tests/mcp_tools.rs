//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every tool, through real MCP clients over stdio and a Unix socket against a real
//! daemon, compared with brute force. Ported from the old `norn-mcp` server's tests.
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
use norn_protocol::common::value::Kind;
use reads_common::{FLOW, MODE, ON, Row, TEMP, history, number, of, seed};
use serde_json::{Value, json};

fn time(second: i64) -> String {
    format!("1970-01-01T00:{:02}:{:02}Z", second / 60, second % 60)
}

fn times(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| time(r.second)).collect()
}

fn seen(rows: &[Value]) -> Vec<String> {
    let at = |r: &Value| r["observeTime"].as_str().unwrap().to_owned();
    rows.iter().map(at).collect()
}

async fn catalogue(c: &Client) {
    let rows = c.ok("entity_service-list_entities_v1", json!({})).await;
    assert_eq!(rows["entities"][0]["name"], "entities/ahu");
    let args = json!({"parent": "entities/-"});
    let rows = c
        .all("signal_service-list_signals_v1", args, "signals", 1)
        .await;
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, [MODE, ON, TEMP, FLOW]);
    let one = c
        .ok("signal_service-get_signal_v1", json!({"name": FLOW}))
        .await;
    assert_eq!(one["stream"], "streams/plant");
    let streams = c.ok("stream_service-list_streams_v1", json!({})).await;
    assert_eq!(streams["streams"][0]["name"], "streams/plant");
    let got = c
        .ok(
            "stream_service-get_stream_v1",
            json!({"name": "streams/plant"}),
        )
        .await;
    assert_eq!(got["name"], "streams/plant");
    let server = c
        .ok("admin_service-get_server_v1", json!({"name": "server"}))
        .await;
    assert_eq!(server["name"], "server");
    let entity = c
        .ok(
            "entity_service-get_entity_v1",
            json!({"name": "entities/pump"}),
        )
        .await;
    assert_eq!(entity["name"], "entities/pump");
}

fn interval(from: i64, to: i64) -> Value {
    json!({"startTime": time(from), "endTime": time(to)})
}

async fn reads(c: &Client) {
    let tool = "query_service-browse_samples_v1";
    let args = json!({"parent": "entities/ahu", "filter": "id = \"temp\"", "interval": interval(1100, 1120)});
    let rows = c.all(tool, args, "samples", 7).await;
    let want = of(TEMP, |r| (1100..1120).contains(&r.second));
    assert_eq!(seen(&rows), times(&want));
    assert_eq!(rows[1]["value"]["doubleValue"], number(&want[1]));

    let args = json!({"parent": "entities/-", "order": "SAMPLE_ORDER_BY_TIME", "interval": interval(1099, 1101)});
    let rows = c.all(tool, args, "samples", 1000).await;
    let mut want: Vec<_> = history()
        .into_iter()
        .filter(|r| (1099..1101).contains(&r.second))
        .collect();
    want.sort_by_key(|r| (r.second, r.sequence, r.signal));
    let names: Vec<_> = rows.iter().map(|r| r["signal"].as_str().unwrap()).collect();
    assert_eq!(names, want.iter().map(|r| r.signal).collect::<Vec<_>>());

    let latest = json!({"aggregate_function": "AGGREGATE_FUNCTION_LATEST"});
    let args = json!({"parent": "entities/-", "aggregation": latest});
    let rows = c.all(tool, args, "aggregates", 1000).await;
    assert_eq!(rows.len(), 4);
    let flow = rows.iter().find(|r| r["signal"] == FLOW).unwrap();
    let newest = of(FLOW, |_| true).pop().unwrap();
    assert_eq!(flow["window"]["startTime"], time(newest.second));
    let Kind::Int64Value(n) = newest.value else {
        panic!()
    };
    assert_eq!(flow["value"]["int64Value"], n.to_string());
}

async fn state(c: &Client) {
    let tool = "state_service-compute_state_v1";
    let args = json!({"parent": "entities/-", "state_time": time(1050)});
    let rows = c.ok(tool, args).await;
    let list = rows["signalStates"].as_array().unwrap();
    assert_eq!(list.len(), 4);
    let flow = list.iter().find(|r| r["signal"] == FLOW).unwrap();
    assert!(flow.get("sample").is_none(), "flow starts at 1100");
    let temp = list.iter().find(|r| r["signal"] == TEMP).unwrap();
    assert_eq!(
        temp["sample"]["observeTime"],
        time(1050),
        "a sample at exactly T counts"
    );
}

async fn computed(c: &Client) {
    let tool = "query_service-find_events_v1";
    let args = json!({"parent": "entities/ahu", "filter": "id = \"temp\"",
        "kind": "EVENT_KIND_MATCH", "sample_filter": "value >= 24"});
    let got = c.all(tool, args, "events", 3).await;
    assert_eq!(got.len(), of(TEMP, |r| number(r) >= 24.0).len());
    let args = json!({"parent": "entities/ahu", "filter": "id = \"on\"", "kind": "EVENT_KIND_VALUE_CHANGE"});
    let rows = of(ON, |_| true);
    let want = rows.windows(2).filter(|w| w[0].value != w[1].value).count();
    assert_eq!(c.all(tool, args, "events", 1).await.len(), want);
    let refused = c
        .fails(
            tool,
            json!({"parent": "entities/-", "kind": "EVENT_KIND_MATCH"}),
        )
        .await;
    assert_eq!(refused["reason"], "ERROR_REASON_INVALID_ARGUMENT");

    let stats = "query_service-compute_stats_v1";
    let args = json!({"parent": "entities/pump", "interval": interval(1100, 1200)});
    let row = c.ok(stats, args).await["signalStats"][0].clone();
    let want = of(FLOW, |r| r.second < 1200);
    assert_eq!(row["sampleCount"], want.len().to_string());
    assert_eq!(row["sum"], want.iter().map(number).sum::<f64>());

    let args = json!({"parent": "entities/pump", "baseline_interval": interval(1100, 1200),
        "comparison_interval": interval(1200, 1400)});
    let row = c.ok("query_service-compare_periods_v1", args).await;
    assert_eq!(row["periodComparisons"][0]["change"]["sampleCount"], "100");
}

#[tokio::test(flavor = "multi_thread")]
async fn every_tool_agrees_with_brute_force_over_stdio_and_a_unix_socket() {
    let mut norn = Norn::both("tools").await;
    seed(&norn.channel).await;
    for client in [norn.unix().await, norn.stdio().await] {
        catalogue(&client).await;
        reads(&client).await;
        state(&client).await;
        computed(&client).await;
    }
    norn.finish().await;
}
