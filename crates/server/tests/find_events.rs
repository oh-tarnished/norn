//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `FindEvents` over real gRPC, against brute force, at several page sizes: a page
//! boundary never hides or invents a change.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;
#[path = "reads_common/mod.rs"]
mod reads_common;

use common::{start, temp};
use norn_protocol::common::QualitySeverity;
use norn_protocol::common::value::Kind;
use norn_protocol::query::{EventKind, FindEventsRequest, SampleEvent};
use norn_server::DaemonConfig;
use query_common::reason;
use reads_common::{MODE, ON, Row, TEMP, number, of, query, second, seed};
use tonic::transport::Channel;

fn request(signal: &str, kind: EventKind, sample_filter: &str) -> FindEventsRequest {
    let (entity, id) = signal.split_once("/signals/").unwrap();
    FindEventsRequest {
        parent: entity.into(),
        filter: format!("id = \"{id}\""),
        kind: kind as i32,
        sample_filter: sample_filter.into(),
        max_value_bytes: 65_536,
        ..FindEventsRequest::default()
    }
}

async fn all(c: &Channel, mut r: FindEventsRequest, size: i32) -> Vec<SampleEvent> {
    r.page_size = size;
    let mut out = Vec::new();
    loop {
        let page = query(c).find_events(r.clone()).await.unwrap().into_inner();
        assert!(page.events.len() <= size as usize);
        out.extend(page.events);
        if page.next_page_token.is_empty() {
            return out;
        }
        r.page_token = page.next_page_token;
    }
}

/// Each change of value in `signal`'s history: (second, value, previous).
fn changes(signal: &str) -> Vec<(i64, Kind, Kind)> {
    let rows = of(signal, |_| true);
    rows.windows(2)
        .filter(|w| w[0].value != w[1].value)
        .map(|w| (w[1].second, w[1].value.clone(), w[0].value.clone()))
        .collect()
}

fn value(e: &SampleEvent) -> Kind {
    e.sample
        .as_ref()
        .unwrap()
        .value
        .clone()
        .unwrap()
        .kind
        .unwrap()
}

#[tokio::test(flavor = "multi_thread")]
async fn matches_and_changes_agree_with_brute_force_at_every_page_size() {
    let d = start(DaemonConfig::default(), temp("find-events")).await;
    seed(&d.channel).await;
    let want: Vec<Row> = of(TEMP, |r| number(r) >= 24.0);
    for size in [1000, 3] {
        let got = all(
            &d.channel,
            request(TEMP, EventKind::Match, "value >= 24"),
            size,
        )
        .await;
        let seconds: Vec<i64> = got
            .iter()
            .map(|e| second(e.sample.as_ref().unwrap()))
            .collect();
        assert_eq!(seconds, want.iter().map(|r| r.second).collect::<Vec<_>>());
        assert_eq!(got[0].kind, EventKind::Match as i32);
    }
    for size in [1000, 1, 4] {
        for signal in [MODE, ON] {
            let got = all(
                &d.channel,
                request(signal, EventKind::ValueChange, ""),
                size,
            )
            .await;
            let want = changes(signal);
            assert_eq!(got.len(), want.len(), "{signal} at page size {size}");
            for (e, (s, v, previous)) in got.iter().zip(&want) {
                assert_eq!(second(e.sample.as_ref().unwrap()), *s);
                assert_eq!(&value(e), v);
                let was = e.previous_value.clone().unwrap().kind.unwrap();
                assert_eq!(&was, previous, "{signal} at page size {size}");
            }
        }
    }
    let rows = of(TEMP, |_| true);
    let flips: Vec<i64> = rows
        .windows(2)
        .filter(|w| w[0].bad != w[1].bad)
        .map(|w| w[1].second)
        .collect();
    for size in [1000, 2] {
        let got = all(
            &d.channel,
            request(TEMP, EventKind::QualityChange, ""),
            size,
        )
        .await;
        let seconds: Vec<i64> = got
            .iter()
            .map(|e| second(e.sample.as_ref().unwrap()))
            .collect();
        assert_eq!(seconds, flips, "at page size {size}");
        let first = got[0].previous_quality.unwrap();
        assert_eq!(
            first.severity,
            QualitySeverity::Bad as i32,
            "sample 0 is BAD"
        );
    }
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_signals_first_sample_is_the_baseline_and_never_a_change() {
    let d = start(DaemonConfig::default(), temp("find-baseline")).await;
    seed(&d.channel).await;
    // ON flips every ten seconds from 1000. An interval starting on a flip sees
    // that sample first, as the baseline: no event for it.
    let mut r = request(ON, EventKind::ValueChange, "");
    r.interval = Some(norn_protocol::google::r#type::Interval {
        start_time: reads_common::at(1_010),
        end_time: reads_common::at(1_031),
    });
    let got = all(&d.channel, r, 100).await;
    let seconds: Vec<i64> = got
        .iter()
        .map(|e| second(e.sample.as_ref().unwrap()))
        .collect();
    assert_eq!(seconds, [1_020, 1_030]);
    d.stop().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_match_needs_a_predicate_and_a_kind_is_required() {
    let d = start(DaemonConfig::default(), temp("find-refused")).await;
    seed(&d.channel).await;
    for r in [
        request(TEMP, EventKind::Match, ""),
        request(TEMP, EventKind::Unspecified, ""),
    ] {
        let refused = query(&d.channel).find_events(r).await.unwrap_err();
        assert_eq!(reason(&refused), "ERROR_REASON_INVALID_ARGUMENT");
    }
    let mut r = request(TEMP, EventKind::ValueChange, "");
    r.page_token = "e1.0000000000000000..s.1.1.entities/dev/signals/temp".into();
    let refused = query(&d.channel).find_events(r).await.unwrap_err();
    assert_eq!(reason(&refused), "ERROR_REASON_INVALID_TOKEN");
    d.stop().await;
}
