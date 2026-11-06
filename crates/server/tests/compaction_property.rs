//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

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
use proptest::test_runner::FileFailurePersistence;
use query_common::{aggregates, all, over, samples};
use tonic::transport::Channel;
use upkeep_common::{compact, segments};

const SIGNALS: [&str; 3] = ["a", "b", "c"];

/// Signal, seconds (out of order and colliding), sequence offset, value bits.
type Row = (usize, i64, i64, u64);

fn wire(i: usize, row: Row) -> Sample {
    let (signal, seconds, sequence, bits) = row;
    Sample {
        signal: format!("entities/dev/signals/{}", SIGNALS.get(signal).unwrap()),
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
            parent: "entities/dev".into(),
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

/// A case the property found in CI: a sample resent after a restart, its original
/// late data written after the signal's latest chunk. The restart's duplicate
/// window missed it, so it was stored twice, and the copy a query returned
/// changed with compaction.
#[test]
fn a_resend_of_late_data_after_a_restart_answers_the_same_before_and_after_compaction() {
    let rounds: Vec<Vec<Row>> = vec![
        vec![(2, 1053, 3, 6483581863293379858u64)],
        vec![
            (1, 1040, 3, 7690607184196536197u64),
            (1, 1058, 3, 15030083407763351227u64),
            (2, 1038, 0, 14914438788150738271u64),
            (1, 1050, 3, 3828873916670400309u64),
            (2, 1052, 2, 15958487554754532712u64),
            (0, 1044, 1, 8948807799430621880u64),
            (2, 1038, 3, 942435919694661410u64),
            (2, 1032, 3, 3245952814042611561u64),
            (0, 1056, 0, 12253245934266425637u64),
            (0, 1038, 2, 1404624780967266346u64),
            (0, 1042, 3, 7870674953713297921u64),
            (2, 1043, 3, 701072306055708450u64),
            (1, 1057, 1, 15276184620895080875u64),
            (0, 1020, 2, 4571469081340756954u64),
            (2, 1023, 0, 16319129816762142731u64),
            (1, 1042, 3, 18338745686356843139u64),
            (1, 1016, 1, 6933341513338030449u64),
            (1, 1026, 3, 9216761102016261243u64),
            (1, 1052, 2, 17749596960104418702u64),
            (0, 1046, 1, 14258332796429975534u64),
            (1, 1016, 1, 14717538604121890589u64),
            (1, 1038, 0, 15845313751834598379u64),
            (0, 1052, 2, 4418858461849780768u64),
            (2, 1017, 2, 17572743851689919225u64),
        ],
        vec![
            (2, 1017, 1, 10234307390331294043u64),
            (1, 1014, 1, 15185419848802271440u64),
            (1, 1041, 1, 3446065687198060849u64),
            (2, 1020, 1, 6941407041731963548u64),
            (2, 1002, 0, 12911679461595734979u64),
            (1, 1017, 2, 7460579218253880935u64),
            (1, 1017, 1, 11373066119323916318u64),
            (1, 1048, 1, 3861894780760714309u64),
            (0, 1056, 1, 6264407085665985925u64),
            (0, 1038, 2, 8214354025345032766u64),
            (2, 1004, 2, 5075806020656598349u64),
            (2, 1017, 0, 660422399701080253u64),
            (2, 1036, 2, 6278579471995027219u64),
            (2, 1044, 3, 17406320122417355143u64),
            (0, 1055, 1, 16866809396077817782u64),
            (1, 1028, 0, 2586694915154023295u64),
            (2, 1035, 2, 7067208591173675196u64),
            (1, 1003, 3, 12730952751010679346u64),
            (0, 1034, 3, 9131774958613959426u64),
        ],
    ];
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(same_answers(rounds));
}

proptest! {
    // Next to this file, so a failure in CI can be replayed: the default looks for
    // a `lib.rs`, which an integration test does not have.
    #![proptest_config(ProptestConfig {
        failure_persistence: Some(Box::new(FileFailurePersistence::WithSource("regressions"))),
        ..ProptestConfig::with_cases(12)
    })]

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
