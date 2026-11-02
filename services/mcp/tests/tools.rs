//! Every tool, over stdio and over HTTP, compared with brute force.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::seed::{FLOW, MODE, ON, Row, TEMP, history, of, seed, time};
use common::{Client, Running, http, stdio, temp};
use serde_json::{Value, json};

fn seconds(rows: &[Value]) -> Vec<String> {
    rows.iter()
        .map(|r| r["time"].as_str().unwrap().to_owned())
        .collect()
}

fn times(rows: &[Row]) -> Vec<String> {
    rows.iter().map(|r| time(r.second)).collect()
}

async fn lists_and_metadata(c: &Client) {
    let names: Vec<String> = c.tools().await.iter().map(|t| t.name.to_string()).collect();
    let mut sorted = names.clone();
    sorted.sort();
    assert_eq!(
        sorted,
        [
            "compare_periods",
            "find_events",
            "get_latest",
            "get_signal_metadata",
            "get_statistics",
            "list_entities",
            "list_signals",
            "query_range",
            "replay",
            "state_at",
        ]
    );
    for tool in c.tools().await {
        let hints = tool.annotations.unwrap();
        assert_eq!(hints.read_only_hint, Some(true), "{}", tool.name);
        assert!(tool.description.unwrap().len() > 80, "{}", tool.name);
    }
    let rows = c.ok("list_entities", json!({})).await;
    assert_eq!(rows["rows"][0]["name"], "entities/ahu");
    assert_eq!(rows["rows"][1]["display_name"], "PUMP");
    let rows = c.all("list_signals", json!({}), 1).await;
    let names: Vec<&str> = rows.iter().map(|r| r["name"].as_str().unwrap()).collect();
    assert_eq!(names, [MODE, ON, TEMP, FLOW]);
    let rows = c
        .ok("list_signals", json!({"target": "entities/pump"}))
        .await;
    assert_eq!(rows["count"], 1);
    let filtered = json!({"filter": "value_type = \"VALUE_TYPE_DOUBLE\""});
    assert_eq!(
        c.ok("list_signals", filtered).await["rows"][0]["name"],
        TEMP
    );
    let meta = c.ok("get_signal_metadata", json!({"signal": FLOW})).await;
    assert_eq!(meta["value_type"], "INT64");
    assert_eq!(meta["unit"], "L/min");
    assert_eq!(meta["stream"], "streams/plant");
}

async fn reads(c: &Client) {
    let args = json!({"target": TEMP, "from": "1100", "to": "1120"});
    let rows = c.all("query_range", args, 1000).await;
    let want = of(TEMP, |r| (1100..1120).contains(&r.second));
    assert_eq!(seconds(&rows), times(&want));
    for (got, want) in rows.iter().zip(&want) {
        assert_eq!(got["value"], want.value);
        assert_eq!(got["sequence"], want.sequence);
        assert_eq!(got["quality"], if want.bad { "BAD" } else { "GOOD" });
    }
    let args = json!({"target": TEMP, "where": "quality.severity = QUALITY_SEVERITY_BAD"});
    let rows = c.all("query_range", args, 1000).await;
    assert_eq!(seconds(&rows), times(&of(TEMP, |r| r.bad)));

    let args = json!({"target": "all", "order": "time", "from": "1099", "to": "1101"});
    let rows = c.all("query_range", args, 1000).await;
    let mut want: Vec<_> = history()
        .into_iter()
        .filter(|r| (1099..1101).contains(&r.second))
        .collect();
    want.sort_by_key(|r| (r.second, r.sequence, r.signal));
    let names: Vec<_> = rows.iter().map(|r| r["signal"].as_str().unwrap()).collect();
    assert_eq!(names, want.iter().map(|r| r.signal).collect::<Vec<_>>());

    let args =
        json!({"target": TEMP, "aggregate": "max", "window": "100s", "from": "1000", "to": "1300"});
    let rows = c.all("query_range", args, 1000).await;
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0]["value"], 24.5);
    assert_eq!(rows[1]["start"], time(1100));
    assert_eq!(rows[1]["sample_count"], 100);

    let rows = c.all("get_latest", json!({"target": "all"}), 1000).await;
    assert_eq!(rows.len(), 4);
    let flow = rows.iter().find(|r| r["signal"] == FLOW).unwrap();
    let newest = of(FLOW, |_| true).pop().unwrap();
    assert_eq!(
        (&flow["time"], &flow["value"]),
        (&json!(time(newest.second)), &newest.value)
    );
    let args = json!({"target": TEMP, "where": "value < 1"});
    let want = of(TEMP, |r| r.value.as_f64().unwrap() < 1.0).pop().unwrap();
    assert_eq!(
        c.ok("get_latest", args).await["rows"][0]["time"],
        time(want.second)
    );
}

async fn state(c: &Client) {
    let result = c
        .ok("state_at", json!({"target": "all", "at": "1050"}))
        .await;
    assert_eq!(result["state_time"], time(1050));
    let rows = result["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 4);
    for signal in [MODE, ON, TEMP] {
        let row = rows.iter().find(|r| r["signal"] == signal).unwrap();
        assert_eq!(row["time"], time(1050), "a sample at exactly T counts");
    }
    let flow = rows.iter().find(|r| r["signal"] == FLOW).unwrap();
    assert_eq!(flow["known"], false, "flow starts at 1100");

    let args = json!({"target": "all", "at": "2000", "max_age": "1m"});
    let rows = c.ok("state_at", args).await;
    let known = rows["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["known"] == true)
        .count();
    assert_eq!(known, 0, "the newest sample is older than max_age");
    let paged = c
        .all("state_at", json!({"target": "all", "at": "1450"}), 3)
        .await;
    assert_eq!(paged.len(), 4);
}

#[tokio::test(flavor = "multi_thread")]
async fn every_read_tool_agrees_with_brute_force_on_both_transports() {
    let daemon = Running::start(temp("tools")).await;
    seed(&daemon.channel().await).await;
    for client in [stdio(&daemon.endpoint).await, http(&daemon.endpoint).await] {
        lists_and_metadata(&client).await;
        reads(&client).await;
        state(&client).await;
        client.service.cancel().await.unwrap();
    }
    daemon.finish().await;
}
