//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `ComputeStats` and `ComparePeriods` over real gRPC, against hand computation.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::float_cmp
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;
#[path = "reads_common/mod.rs"]
mod reads_common;

use common::{start, temp};
use norn_protocol::common::Value;
use norn_protocol::common::value::Kind;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{ComparePeriodsRequest, ComputeStatsRequest, SignalStats};
use norn_server::DaemonConfig;
use query_common::reason;
use reads_common::{FLOW, MODE, ON, Row, TEMP, at, number, of, query, seed};

fn between(from: i64, to: i64) -> Option<Interval> {
    Some(Interval {
        start_time: at(from),
        end_time: at(to),
    })
}

fn kind(v: Option<&Value>) -> Kind {
    v.unwrap().kind.clone().unwrap()
}

fn as_number(v: Option<&Value>) -> f64 {
    match kind(v) {
        Kind::DoubleValue(n) => n,
        Kind::Int64Value(n) => n as f64,
        other => panic!("{other:?}"),
    }
}

fn check(row: &SignalStats, want: &[Row]) {
    let values: Vec<f64> = want.iter().map(number).collect();
    let sum: f64 = values.iter().sum();
    assert_eq!(row.sample_count, want.len() as i64);
    assert_eq!(row.sum.unwrap(), sum);
    assert_eq!(row.average.unwrap(), sum / values.len() as f64);
    assert_eq!(
        as_number(row.min_value.as_ref()),
        values.iter().copied().fold(f64::MAX, f64::min)
    );
    assert_eq!(
        as_number(row.max_value.as_ref()),
        values.iter().copied().fold(f64::MIN, f64::max)
    );
    let (first, last) = (want.first().unwrap(), want.last().unwrap());
    assert_eq!(row.first_time, at(first.second));
    assert_eq!(kind(row.first_value.as_ref()), first.value);
    assert_eq!(row.last_time, at(last.second));
    assert_eq!(kind(row.last_value.as_ref()), last.value);
}

fn stats(parent: &str, filter: &str) -> ComputeStatsRequest {
    ComputeStatsRequest {
        parent: parent.into(),
        filter: filter.into(),
        ..ComputeStatsRequest::default()
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn statistics_agree_with_hand_computation_across_pages() {
    let d = start(DaemonConfig::default(), temp("stats")).await;
    seed(&d.channel).await;
    let mut r = stats("entities/dev", "id = \"temp\"");
    r.interval = between(1_000, 1_200);
    let page = query(&d.channel)
        .compute_stats(r)
        .await
        .unwrap()
        .into_inner();
    check(&page.signal_stats[0], &of(TEMP, |r| r.second < 1_200));

    let mut r = stats("entities/-", "");
    r.page_size = 2;
    let mut rows = Vec::new();
    loop {
        let page = query(&d.channel)
            .compute_stats(r.clone())
            .await
            .unwrap()
            .into_inner();
        rows.extend(page.signal_stats);
        if page.next_page_token.is_empty() {
            break;
        }
        r.page_token = page.next_page_token;
    }
    let names: Vec<&str> = rows.iter().map(|r| r.signal.as_str()).collect();
    assert_eq!(names, [MODE, ON, TEMP, FLOW]);
    assert!(rows[1].average.is_none(), "a bool has no average");
    assert_eq!(kind(rows[1].min_value.as_ref()), Kind::BoolValue(false));
    assert!(
        rows[0].value_truncated,
        "mode's longest string is its maximum"
    );
    check(&rows[3], &of(FLOW, |_| true));

    let mut r = stats("entities/dev", "id = \"temp\"");
    r.interval = between(5_000, 5_001);
    let empty = &query(&d.channel)
        .compute_stats(r)
        .await
        .unwrap()
        .into_inner()
        .signal_stats[0];
    assert_eq!((empty.sample_count, empty.min_value.is_none()), (0, true));
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_comparison_is_both_periods_and_their_difference() {
    let d = start(DaemonConfig::default(), temp("compare")).await;
    seed(&d.channel).await;
    let r = ComparePeriodsRequest {
        parent: "entities/pump".into(),
        baseline_interval: between(1_100, 1_200),
        comparison_interval: between(1_200, 1_400),
        ..ComparePeriodsRequest::default()
    };
    let page = query(&d.channel)
        .compare_periods(r.clone())
        .await
        .unwrap()
        .into_inner();
    let row = &page.period_comparisons[0];
    let a = of(FLOW, |r| r.second < 1_200);
    let b = of(FLOW, |r| (1_200..1_400).contains(&r.second));
    check(row.baseline.as_ref().unwrap(), &a);
    check(row.comparison.as_ref().unwrap(), &b);
    let mean = |rows: &[Row]| rows.iter().map(number).sum::<f64>() / rows.len() as f64;
    let change = row.change.unwrap();
    assert_eq!(change.average.unwrap(), mean(&b) - mean(&a));
    assert_eq!(change.sample_count, 100);
    let r2 = ComparePeriodsRequest {
        parent: "entities/dev".into(),
        filter: "id = \"on\"".into(),
        baseline_interval: between(1_000, 1_010),
        comparison_interval: between(1_010, 1_020),
        ..ComparePeriodsRequest::default()
    };
    let page = query(&d.channel)
        .compare_periods(r2)
        .await
        .unwrap()
        .into_inner();
    assert!(page.period_comparisons[0].change.unwrap().average.is_none());
    let open = ComparePeriodsRequest {
        baseline_interval: Some(Interval {
            start_time: at(1_100),
            end_time: None,
        }),
        ..r
    };
    let refused = query(&d.channel).compare_periods(open).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_INVALID_ARGUMENT");
    d.stop().await;
}
