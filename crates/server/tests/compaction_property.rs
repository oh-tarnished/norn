//! A query's answer is identical before and after compaction, on generated data.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;
mod query_common;
mod upkeep_common;

use common::{append, batch, seed, start, temp};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::query::{AggregateFunction, QuerySamplesRequest};
use norn_protocol::state::ComputeStateRequest;
use norn_protocol::state::state_service_client::StateServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Persisted;
use norn_server::DaemonConfig;
use proptest::prelude::*;
use query_common::{aggregates, all, over, samples};
use tonic::transport::Channel;
use upkeep_common::{compact, segments};

const SIGNALS: [&str; 3] = ["a", "b", "c"];

/// Signal, seconds (out of order and colliding), sequence offset, value bits.
type Row = (usize, i64, i64, u64);

fn wire(i: usize, row: Row) -> Sample {
    let (signal, seconds, sequence, bits) = row;
    Sample {
        signal: format!("entities/ahu/signals/{}", SIGNALS.get(signal).unwrap()),
        observe_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(f64::from_bits(bits))),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: sequence + i as i64,
    }
}

async fn answers(channel: &Channel) -> Vec<String> {
    let mut out = Vec::new();
    let by_time = QuerySamplesRequest {
        order: norn_protocol::query::SampleOrder::ByTime as i32,
        ..all()
    };
    out.push(format!("{:?}", samples(channel, all()).await));
    out.push(format!("{:?}", samples(channel, by_time).await));
    for f in [
        AggregateFunction::Count,
        AggregateFunction::Latest,
        AggregateFunction::First,
    ] {
        out.push(format!("{:?}", aggregates(channel, over(f, 7)).await));
    }
    for seconds in [1_005, 1_050, 2_000] {
        let request = ComputeStateRequest {
            parent: "entities/ahu".into(),
            state_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
            ..ComputeStateRequest::default()
        };
        let mut client = StateServiceClient::new(channel.clone());
        let got = client.compute_state(request).await.unwrap().into_inner();
        out.push(format!("{:?}", got.signal_states));
    }
    out
}

async fn same_answers(rounds: Vec<Vec<Row>>) {
    let kinds = SIGNALS.map(|s| (s, ValueType::Double));
    let d = start(DaemonConfig::default(), temp("compact-prop")).await;
    seed(&d.channel, &kinds).await;
    let mut data = d.stop_keeping().await;
    let mut offset = 0;
    for rows in rounds {
        let d = start(DaemonConfig::default(), data).await;
        let list: Vec<Sample> = rows
            .iter()
            .enumerate()
            .map(|(i, row)| wire(offset + i, *row))
            .collect();
        offset += list.len();
        append(&d.channel, vec![batch(1, list, Persisted)]).await;
        data = d.stop_keeping().await;
    }
    let d = start(DaemonConfig::default(), data).await;
    let before = answers(&d.channel).await;
    compact(&d.channel).await;
    assert_eq!(segments(&d.data).len(), 1);
    let after = answers(&d.channel).await;
    let data = d.stop_keeping().await;
    let d = start(DaemonConfig::default(), data).await;
    let restarted = answers(&d.channel).await;
    d.stop().await;
    assert_eq!(before, after);
    assert_eq!(before, restarted);
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(12))]

    #[test]
    fn a_querys_answer_is_identical_before_and_after_compaction(
        rounds in prop::collection::vec(
            prop::collection::vec(
                (0usize..3, 1_000i64..1_060, 0i64..4, any::<u64>()),
                1..40,
            ),
            2..5,
        )
    ) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .unwrap();
        runtime.block_on(same_answers(rounds));
    }
}
