//! Crashes and damage, seen through a query.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;
#[path = "query_common/mod.rs"]
mod query_common;

use std::collections::BTreeSet;
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{Requests, append, batch, double, seed, start, temp};
use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::common::{ErrorReason, ValueType};
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel::Durable;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_server::DaemonConfig;
use norn_storage::{SegmentReader, StorageOptions, live_segments};
use query_common::{all, pages, reason, samples};
use tokio::sync::mpsc;
use tonic::Code;
use tonic::transport::Channel;

fn spawn(data: &Path, socket: &Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(data)
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .args(["--log-level", "error"])
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

async fn connect(socket: &Path) -> Channel {
    let endpoint = Endpoint::Unix(socket.to_path_buf());
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let get = GetServerRequest {
                name: "server".into(),
            };
            if AdminServiceClient::new(channel.clone())
                .get_server(get)
                .await
                .is_ok()
            {
                return channel;
            }
        }
        assert!(Instant::now() < deadline, "the daemon never answered");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_daemon_killed_with_kill_9_then_queried_returns_every_durable_sample() {
    const BATCH: i64 = 100;
    let root = temp("query-kill");
    let (data, socket) = (root.join("data"), root.join("ipc/n.sock"));
    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    seed(&channel, &[("t", ValueType::Double)]).await;
    let (tx, rx) = mpsc::channel(32);
    tokio::spawn(async move {
        for id in 1.. {
            let from = (id - 1) * BATCH;
            let list = (from..from + BATCH).map(|n| double("t", n)).collect();
            if tx.send(batch(id, list, Durable)).await.is_err() {
                return;
            }
        }
    });
    let mut client = TelemetryServiceClient::new(channel);
    let mut replies = client
        .append_samples(Requests(rx))
        .await
        .unwrap()
        .into_inner();
    let mut acked = BTreeSet::new();
    let reader = tokio::spawn(async move {
        while let Ok(Some(r)) = replies.message().await {
            if r.ack_level == Durable as i32 && r.error.is_none() {
                acked.insert(r.batch_id);
            }
        }
        acked
    });
    tokio::time::sleep(Duration::from_millis(400)).await;
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let acked = reader.await.unwrap();
    assert!(!acked.is_empty(), "nothing was acknowledged");

    let mut daemon = spawn(&data, &socket);
    let channel = connect(&socket).await;
    let got = samples(&channel, all()).await;
    let seqs: Vec<i64> = got.iter().map(|s| s.sequence).collect();
    assert!(
        seqs.windows(2).all(|w| w[0] < w[1]),
        "a sample came back twice"
    );
    let present: BTreeSet<i64> = seqs.into_iter().collect();
    for id in acked {
        for n in (id - 1) * BATCH..id * BATCH {
            assert!(present.contains(&n), "sample {n} of batch {id} was lost");
        }
    }
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupt_chunk_is_a_typed_error_and_its_samples_leave_the_count() {
    let root = temp("query-corrupt");
    let d = start(DaemonConfig::default(), root.clone()).await;
    seed(&d.channel, &[("t", ValueType::Double)]).await;
    let list = (0..50).map(|n| double("t", n)).collect();
    append(&d.channel, vec![batch(1, list, Durable)]).await;
    let data = d.stop_keeping().await;

    let stream_dir = std::fs::read_dir(data.join("streams"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap();
    let segment = live_segments(&stream_dir.path()).unwrap().remove(0).path;
    let reader = SegmentReader::open(&segment, StorageOptions::default()).unwrap();
    let chunk = reader.chunks()[0];
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .open(&segment)
        .unwrap();
    file.seek(SeekFrom::Start(chunk.offset + chunk.frame_len() - 1))
        .unwrap();
    file.write_all(&[0x5a]).unwrap();
    drop(file);

    let d = start(DaemonConfig::default(), data).await;
    let refused = pages(&d.channel, all()).await.unwrap_err();
    assert_eq!(refused.code(), Code::DataLoss);
    assert_eq!(reason(&refused), ErrorReason::DataCorrupted.as_str_name());
    use norn_protocol::rpc::StatusExt;
    let info = refused.get_details_error_info().unwrap();
    assert!(info.metadata["chunk"].contains(".nseg@"), "{info:?}");
    assert!(info.metadata.contains_key("min_time"));

    let name = "streams/plant".to_owned();
    let stream = StreamServiceClient::new(d.channel.clone())
        .get_stream(GetStreamRequest { name })
        .await
        .unwrap()
        .into_inner();
    let stats = stream.stats.unwrap();
    assert_eq!((stats.sample_count, stats.corrupt_sample_count), (0, 50));
    let data = d.stop_keeping().await;
    let _ = std::fs::remove_dir_all(data);
}
