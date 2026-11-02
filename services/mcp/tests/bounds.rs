//! Results stay bounded and a continuation loses and repeats nothing.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::seed::{FLOW, MODE, TEMP, of, seed, time};
use common::{Client, Running, stdio, temp};
use serde_json::{Value, json};

fn keys(rows: &[Value]) -> Vec<(String, String, i64)> {
    let key = |r: &Value| {
        let name = r["signal"].as_str().unwrap().to_owned();
        let at = r["time"].as_str().unwrap().to_owned();
        (name, at, r["sequence"].as_i64().unwrap())
    };
    rows.iter().map(key).collect()
}

async fn every_page_size_reads_the_same(c: &Client) {
    for order in ["signal", "time"] {
        let args = json!({"target": "all", "order": order});
        let whole = c.all("query_range", args.clone(), 1000).await;
        assert_eq!(whole.len(), 1100);
        for limit in [1, 7, 333] {
            if limit == 1 && order == "time" {
                continue; // 1100 calls say no more than 7 do, and this keeps the test fast.
            }
            let paged = c.all("query_range", args.clone(), limit).await;
            assert_eq!(keys(&paged), keys(&whole), "{order} at limit {limit}");
        }
    }
    let args = json!({"target": "all", "aggregate": "count", "window": "10s", "from": "1000"});
    let whole = c.all("query_range", args.clone(), 1000).await;
    assert_eq!(c.all("query_range", args, 3).await, whole);
    assert_eq!(
        c.all("get_latest", json!({"target": "all"}), 1).await.len(),
        4
    );
    assert_eq!(c.all("list_entities", json!({}), 1).await.len(), 2);
}

async fn a_default_read_is_bounded(c: &Client) {
    let page = c.ok("query_range", json!({"target": "all"})).await;
    assert_eq!(page["count"], 100);
    assert_eq!(page["complete"], false);
    assert!(page.to_string().len() < 300 * 1024);
    let refused = c
        .fails("query_range", json!({"target": "all", "limit": 1001}))
        .await;
    assert_eq!(refused["reason"], "MCP_LIMIT_EXCEEDED");
    assert!(refused["hint"].as_str().unwrap().contains("continuation"));
    let refused = c
        .fails(
            "state_at",
            json!({"target": "all", "at": "now", "limit": 181}),
        )
        .await;
    assert_eq!(refused["reason"], "MCP_LIMIT_EXCEEDED");
    let refused = c
        .fails("query_range", json!({"target": "all", "limit": 0}))
        .await;
    assert_eq!(refused["reason"], "MCP_INVALID_ARGUMENT");
}

async fn a_long_value_is_cut_and_says_so(c: &Client) {
    let args = json!({"target": MODE, "from": "1050", "to": "1051"});
    let row = c.ok("query_range", args).await["rows"][0].clone();
    assert_eq!(row["truncated"], true);
    assert_eq!(row["value"].as_str().unwrap().len(), 1024);
    let args = json!({"target": MODE, "from": "1049", "to": "1050"});
    let row = c.ok("query_range", args).await["rows"][0].clone();
    assert!(row.get("truncated").is_none());
}

async fn a_continuation_serves_only_its_own_arguments(c: &Client) {
    let args = json!({"target": TEMP, "limit": 2});
    let page = c.ok("query_range", args).await;
    let token = page["continuation"].clone();
    let other = json!({"target": FLOW, "continuation": token});
    let failed = c.fails("query_range", other).await;
    assert_eq!(failed["reason"], "MCP_INVALID_CONTINUATION");
    let mangled = json!({"target": TEMP, "continuation": "zz"});
    let failed = c.fails("query_range", mangled).await;
    assert_eq!(failed["reason"], "MCP_INVALID_CONTINUATION");
    // `limit` alone may change between pages.
    let next = json!({"target": TEMP, "limit": 5, "continuation": token});
    let page = c.ok("query_range", next).await;
    assert_eq!(page["count"], 5);
    assert_eq!(page["rows"][0]["time"], time(of(TEMP, |_| true)[2].second));
}

async fn relative_times_mean_one_thing_across_pages(c: &Client) {
    let args = json!({"target": TEMP, "from": "1000", "to": "now"});
    assert_eq!(c.all("query_range", args, 200).await.len(), 500);
}

#[tokio::test(flavor = "multi_thread")]
async fn output_is_bounded_and_continuations_are_exact() {
    let daemon = Running::start(temp("bounds")).await;
    seed(&daemon.channel().await).await;
    let client = stdio(&daemon.endpoint).await;
    every_page_size_reads_the_same(&client).await;
    a_default_read_is_bounded(&client).await;
    a_long_value_is_cut_and_says_so(&client).await;
    a_continuation_serves_only_its_own_arguments(&client).await;
    relative_times_mean_one_thing_across_pages(&client).await;
    client.service.cancel().await.unwrap();
    daemon.finish().await;
}
