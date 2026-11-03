//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `FindEvents` compares values bit for bit, across page boundaries too.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{append, batch, double, seed, start, temp};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Value, ValueType};
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{EventKind, FindEventsRequest};
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_server::DaemonConfig;

#[tokio::test(flavor = "multi_thread")]
async fn a_change_is_bit_for_bit_so_signed_zero_differs_and_nan_equals_itself() {
    let d = start(DaemonConfig::default(), temp("find-bits")).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let nan = f64::from_bits(0x7ff8_0000_0000_0001);
    let values = [0.0, -0.0, -0.0, nan, nan, f64::NAN, 1.0];
    let samples = values.iter().enumerate().map(|(i, v)| {
        let mut s = double("t", i as i64);
        s.value = Some(Value {
            kind: Some(Kind::DoubleValue(*v)),
        });
        s
    });
    append(&d.channel, vec![batch(1, samples.collect(), Persisted)]).await;
    for size in [100, 1] {
        let mut r = FindEventsRequest {
            parent: "entities/ahu".into(),
            kind: EventKind::ValueChange as i32,
            page_size: size,
            ..FindEventsRequest::default()
        };
        let mut at = Vec::new();
        loop {
            let mut client = QueryServiceClient::new(d.channel.clone());
            let page = client.find_events(r.clone()).await.unwrap().into_inner();
            let seconds = page
                .events
                .iter()
                .map(|e| e.sample.as_ref().unwrap().observe_time.unwrap().seconds);
            at.extend(seconds.map(|s| s - 1_000));
            if page.next_page_token.is_empty() {
                break;
            }
            r.page_token = page.next_page_token;
        }
        // -0.0 after 0.0; the first NaN; a NaN of another payload; 1.0.
        assert_eq!(at, [1, 3, 5, 6], "at page size {size}");
    }
    d.stop().await;
}
