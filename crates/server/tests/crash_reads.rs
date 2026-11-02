//! After `kill -9`, StateAt and replay both return every sample acknowledged `DURABLE`.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
mod events_common;
mod process;

use std::collections::BTreeSet;
use std::time::Duration;

use common::{seed, temp};
use events_common::{create, kind, replay, replays, sample, subscribe, until};
use norn_protocol::common::ValueType;
use norn_protocol::replay::ResumeReplaySessionRequest;
use norn_protocol::replay::replay_session::Pace;
use norn_protocol::state::ComputeStateRequest;
use norn_protocol::state::state_service_client::StateServiceClient;
use norn_protocol::telemetry::ClockEventKind;
use process::{connect, ingest_until_killed, spawn};

const BATCH: i64 = 100;

#[tokio::test(flavor = "multi_thread")]
async fn after_kill_9_state_and_replay_return_every_durable_sample() {
    let root = temp("state-kill");
    let (data, socket) = (root.join("data"), root.join("ipc/n.sock"));
    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    seed(&channel, &[("t", ValueType::Double)]).await;
    let run = tokio::spawn(ingest_until_killed(channel, "t", BATCH));
    tokio::time::sleep(Duration::from_millis(400)).await;
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let acked = run.await.unwrap();
    assert!(!acked.is_empty(), "nothing was acknowledged");

    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    let mut client = StateServiceClient::new(channel.clone());
    // Every durable sample is the state at its own instant: the newest at or before it.
    for id in &acked {
        for n in [(id - 1) * BATCH, id * BATCH - 1] {
            let request = ComputeStateRequest {
                parent: "entities/ahu".into(),
                state_time: Some(prost_types::Timestamp {
                    seconds: 1_000 + n,
                    nanos: 0,
                }),
                ..ComputeStateRequest::default()
            };
            let got = client.compute_state(request).await.unwrap().into_inner();
            let sample = got.signal_states[0].sample.as_ref();
            assert_eq!(
                sample.map(|s| s.sequence),
                Some(n),
                "sample {n} of batch {id}"
            );
        }
    }
    // A replay of everything holds every durable sample, once, in order.
    let last = acked.iter().max().copied().unwrap() * BATCH;
    let session = create(&channel, 1_000, 1_000 + last + 1, Pace::MaxSpeed(true)).await;
    let mut events = subscribe(&channel, replay(&session.name)).await.unwrap();
    let resume = ResumeReplaySessionRequest {
        name: session.name,
        etag: String::new(),
    };
    replays(&channel)
        .resume_replay_session(resume)
        .await
        .unwrap();
    let got = until(&mut events, 0, |e| kind(e) == Some(ClockEventKind::Ended)).await;
    let seqs: Vec<i64> = got.iter().filter_map(sample).map(|s| s.sequence).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "out of order or twice"
    );
    let present: BTreeSet<i64> = seqs.into_iter().collect();
    for id in &acked {
        for n in (id - 1) * BATCH..id * BATCH {
            assert!(present.contains(&n), "replay lost sample {n} of batch {id}");
        }
    }
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
