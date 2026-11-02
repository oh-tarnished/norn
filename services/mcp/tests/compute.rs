//! find_events, the statistics and replay, over a real daemon, against brute force.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::seed::{FLOW, MODE, ON, Row, TEMP, of, seed, time};
use common::{Client, Running, stdio, temp};
use serde_json::{Value, json};

fn number(r: &Row) -> f64 {
    r.value.as_f64().unwrap()
}

async fn events(c: &Client) {
    let want = of(TEMP, |r| number(r) >= 24.0);
    for limit in [1000, 3] {
        let args = json!({"target": TEMP, "kind": "match", "where": "value >= 24"});
        let rows = c.all("find_events", args, limit).await;
        assert_eq!(rows.len(), want.len());
        assert_eq!(rows[1]["time"], time(want[1].second));
        assert_eq!(rows[0]["event"], "match");
    }
    // A hand-rolled change detector, per signal.
    let changes = |signal: &str| -> Vec<(i64, Value, Value)> {
        let rows = of(signal, |_| true);
        rows.windows(2)
            .filter(|w| w[0].value != w[1].value)
            .map(|w| (w[1].second, w[1].value.clone(), w[0].value.clone()))
            .collect()
    };
    let want = [changes(MODE), changes(ON)].concat();
    for limit in [1000, 1, 4] {
        let mut got = Vec::new();
        for signal in [MODE, ON] {
            let args = json!({"target": signal, "kind": "value_change"});
            got.extend(c.all("find_events", args, limit).await);
        }
        assert_eq!(got.len(), want.len(), "at limit {limit}");
        for (row, (second, value, previous)) in got.iter().zip(&want) {
            assert_eq!(row["time"], time(*second));
            if !value.as_str().is_some_and(|v| v.len() > 1_024) {
                assert_eq!(&row["value"], value);
            }
            if !previous.as_str().is_some_and(|v| v.len() > 1_024) {
                assert_eq!(&row["previous"], previous, "at limit {limit}");
            }
        }
    }
    let flips = of(TEMP, |_| true);
    let want: Vec<i64> = flips
        .windows(2)
        .filter(|w| w[0].bad != w[1].bad)
        .map(|w| w[1].second)
        .collect();
    let args = json!({"target": TEMP, "kind": "quality_change"});
    for limit in [1000, 2] {
        let rows = c.all("find_events", args.clone(), limit).await;
        let got: Vec<&str> = rows.iter().map(|r| r["time"].as_str().unwrap()).collect();
        assert_eq!(got, want.iter().map(|s| time(*s)).collect::<Vec<_>>());
    }
    assert_eq!(c.all("find_events", args, 1000).await[0]["previous"], "BAD");
    let failed = c
        .fails("find_events", json!({"target": TEMP, "kind": "match"}))
        .await;
    assert_eq!(failed["reason"], "MCP_INVALID_ARGUMENT");
}

fn check_stats(row: &Value, want: &[Row]) {
    let values: Vec<f64> = want.iter().map(number).collect();
    let sum: f64 = values.iter().sum();
    assert_eq!(row["sample_count"], want.len());
    assert_eq!(row["sum"].as_f64().unwrap(), sum);
    assert_eq!(row["average"].as_f64().unwrap(), sum / values.len() as f64);
    assert_eq!(
        row["min"].as_f64().unwrap(),
        values.iter().copied().fold(f64::MAX, f64::min)
    );
    assert_eq!(
        row["max"].as_f64().unwrap(),
        values.iter().copied().fold(f64::MIN, f64::max)
    );
    let (first, last) = (want.first().unwrap(), want.last().unwrap());
    assert_eq!(row["first"]["time"], time(first.second));
    assert_eq!(row["first"]["value"], first.value);
    assert_eq!(row["last"]["time"], time(last.second));
    assert_eq!(row["last"]["value"], last.value);
}

async fn statistics(c: &Client) {
    let args = json!({"target": TEMP, "from": "1000", "to": "1200"});
    let row = &c.ok("get_statistics", args).await["rows"][0];
    check_stats(row, &of(TEMP, |r| r.second < 1200));

    let rows = c.all("get_statistics", json!({"target": "all"}), 2).await;
    let names: Vec<&str> = rows.iter().map(|r| r["signal"].as_str().unwrap()).collect();
    assert_eq!(names, [MODE, ON, TEMP, FLOW]);
    assert!(rows[1].get("average").is_none(), "a bool has no average");
    assert_eq!(rows[1]["min"], false);
    check_stats(&rows[3], &of(FLOW, |_| true));

    let args = json!({"target": TEMP, "from": "5000", "to": "5001"});
    assert_eq!(
        c.ok("get_statistics", args).await["rows"][0]["sample_count"],
        0
    );
    let failed = c
        .fails("get_statistics", json!({"target": "all", "limit": 51}))
        .await;
    assert_eq!(failed["reason"], "MCP_LIMIT_EXCEEDED");
}

async fn compare(c: &Client) {
    let args = json!({
        "target": FLOW,
        "a": {"from": "1100", "to": "1200"},
        "b": {"from": "1200", "to": "1400"},
    });
    let row = &c.ok("compare_periods", args).await["rows"][0];
    let a = of(FLOW, |r| r.second < 1200);
    let b = of(FLOW, |r| (1200..1400).contains(&r.second));
    check_stats(&row["a"], &a);
    check_stats(&row["b"], &b);
    let mean = |rows: &[Row]| rows.iter().map(number).sum::<f64>() / rows.len() as f64;
    assert_eq!(
        row["change"]["average"].as_f64().unwrap(),
        mean(&b) - mean(&a)
    );
    assert_eq!(row["change"]["sample_count"].as_f64().unwrap(), 100.0);
    let args = json!({
        "target": "entities/ahu/signals/on",
        "a": {"from": "1000", "to": "1010"},
        "b": {"from": "1010", "to": "1020"},
    });
    let change = c.ok("compare_periods", args).await;
    assert!(change["rows"][0]["change"].get("average").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn events_statistics_and_comparisons_agree_with_brute_force() {
    let daemon = Running::start(temp("compute")).await;
    seed(&daemon.channel().await).await;
    let client = stdio(&daemon.endpoint).await;
    events(&client).await;
    statistics(&client).await;
    compare(&client).await;
    client.service.cancel().await.unwrap();
    daemon.finish().await;
}
