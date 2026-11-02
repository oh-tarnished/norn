//! The ways a call fails, as a model sees them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

use common::seed::{FLOW, TEMP, seed, time};
use common::{Running, stdio, temp};
use norn_storage::{SegmentReader, StorageOptions, live_segments};
use serde_json::json;

/// Flips a byte of the first chunk of every stream on disk.
fn damage(data: &Path) {
    let streams = std::fs::read_dir(data.join("streams")).unwrap();
    let mut damaged = 0;
    for dir in streams {
        for segment in live_segments(&dir.unwrap().path()).unwrap() {
            let reader = SegmentReader::open(&segment.path, StorageOptions::default()).unwrap();
            let chunk = reader.chunks()[0];
            let mut file = std::fs::OpenOptions::new()
                .write(true)
                .open(&segment.path)
                .unwrap();
            file.seek(SeekFrom::Start(chunk.offset + chunk.frame_len() - 1))
                .unwrap();
            file.write_all(&[0x5a]).unwrap();
            damaged += 1;
        }
    }
    assert!(
        damaged > 0,
        "nothing was damaged, so nothing would be proved"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn a_damaged_chunk_is_a_tool_error_naming_what_to_read_around() {
    let daemon = Running::start(temp("loss")).await;
    seed(&daemon.channel().await).await;
    let root = daemon.stop().await;
    damage(&root.join("data"));
    let daemon = Running::start(root).await;
    let c = stdio(&daemon.endpoint).await;
    for (tool, args) in [
        ("query_range", json!({"target": "all"})),
        ("get_statistics", json!({"target": "all"})),
        (
            "find_events",
            json!({"target": "all", "kind": "value_change"}),
        ),
    ] {
        let failed = c.fails(tool, args).await;
        assert_eq!(failed["reason"], "ERROR_REASON_DATA_CORRUPTED", "{tool}");
        assert!(failed["message"].as_str().unwrap().contains("QuerySamples"));
        assert!(
            failed["metadata"]["chunk"]
                .as_str()
                .unwrap()
                .contains(".nseg@")
        );
        assert!(
            failed["hint"]
                .as_str()
                .unwrap()
                .contains("Read around the damage")
        );
        assert_eq!(failed["retryable"], false);
    }
    c.service.cancel().await.unwrap();
    daemon.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn bad_input_is_a_tool_error_naming_the_argument() {
    let daemon = Running::start(temp("input")).await;
    seed(&daemon.channel().await).await;
    let c = stdio(&daemon.endpoint).await;
    let cases = [
        (
            "query_range",
            json!({"target": TEMP, "from": "yesterday"}),
            "MCP_INVALID_ARGUMENT",
        ),
        (
            "query_range",
            json!({"target": "BAD NAME"}),
            "MCP_INVALID_ARGUMENT",
        ),
        (
            "query_range",
            json!({"target": TEMP, "window": "1m"}),
            "MCP_INVALID_ARGUMENT",
        ),
        ("query_range", json!({}), "MCP_INVALID_ARGUMENT"),
        (
            "query_range",
            json!({"target": TEMP, "nonsense": 1}),
            "MCP_INVALID_ARGUMENT",
        ),
        (
            "query_range",
            json!({"target": "all", "where": "value >"}),
            "ERROR_REASON_INVALID_ARGUMENT",
        ),
        (
            "query_range",
            json!({"target": "all", "where": "value > 20"}),
            "ERROR_REASON_INVALID_ARGUMENT",
        ),
        (
            "query_range",
            json!({"target": "entities/none"}),
            "ERROR_REASON_RESOURCE_NOT_FOUND",
        ),
        (
            "get_signal_metadata",
            json!({"signal": "entities/ahu/signals/none"}),
            "ERROR_REASON_RESOURCE_NOT_FOUND",
        ),
        (
            "replay",
            json!({"action": "start", "target": TEMP}),
            "MCP_INVALID_ARGUMENT",
        ),
        ("replay", json!({"action": "step"}), "MCP_INVALID_ARGUMENT"),
    ];
    for (tool, args, reason) in cases {
        let failed = c.fails(tool, args.clone()).await;
        assert_eq!(failed["reason"], reason, "{tool} {args}: {failed}");
    }
    let missing = c
        .fails("query_range", json!({"target": "entities/none"}))
        .await;
    assert!(missing["hint"].as_str().unwrap().contains("list_entities"));
    let unknown = rmcp::model::CallToolRequestParams::new("delete_everything");
    assert!(
        c.service.call_tool(unknown).await.is_err(),
        "no such tool is a protocol error"
    );
    c.service.cancel().await.unwrap();
    daemon.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_that_is_down_goes_away_or_comes_back_is_handled() {
    let root = temp("down");
    let endpoint = common::socket(&root);
    // norn-mcp starts with no daemon there, and says so per call.
    let c = stdio(&endpoint).await;
    assert_eq!(c.tools().await.len(), 10, "the tool list needs no daemon");
    let failed = c.fails("list_entities", json!({})).await;
    assert_eq!(failed["reason"], "DAEMON_UNAVAILABLE");
    assert_eq!(failed["retryable"], true);
    assert!(failed["hint"].as_str().unwrap().contains("nornctl status"));

    let daemon = Running::start(root).await;
    seed(&daemon.channel().await).await;
    let args = json!({"target": FLOW, "from": "1100", "to": "1101"});
    let row = c.ok("query_range", args.clone()).await["rows"][0].clone();
    assert_eq!(row["time"], time(1100));

    let root = daemon.stop().await;
    let failed = c.fails("query_range", args.clone()).await;
    assert_eq!(failed["reason"], "DAEMON_UNAVAILABLE", "{failed}");

    let daemon = Running::start(root).await;
    let row = c.ok("query_range", args).await["rows"][0].clone();
    assert_eq!(row["value"], 0);
    c.service.cancel().await.unwrap();
    daemon.finish().await;
}
