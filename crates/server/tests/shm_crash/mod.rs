//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Child writers for the crash tests, and a publisher a test controls fully,
//! opened as `norn_shared_memory::Producer` opens one, which `Producer` will not
//! let a test stop halfway through a frame.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};

use iceoryx2::config::Config;
use iceoryx2::port::publisher::Publisher;
use iceoryx2::prelude::*;
use iceoryx2::service::port_factory::publish_subscribe::PortFactory;
use norn_ipc::{Endpoint, SharedMemoryTicket};
use norn_protocol::telemetry::AppendSamplesResponse;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::{
    DATA_PLANE_METADATA_KEY, DATA_PLANE_SHARED_MEMORY, SHARED_MEMORY_SERVICE_METADATA_KEY,
};
use tokio::sync::mpsc;
use tonic::Streaming;
use tonic::metadata::MetadataValue;
use tonic::transport::Channel;

use super::common::{Requests, Running};

type Flavour = ipc_threadsafe::Service;

/// Runs `test` as a child of this binary, against the daemon `running` serves.
pub fn child(running: &Running, test: &str) -> Child {
    let socket = running.data.parent().unwrap().join("ipc/n.sock");
    Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture", "--test-threads=1"])
        .env("NORN_SHM_CHILD", socket)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// The socket to write to, when this process is a child.
pub fn target() -> Option<PathBuf> {
    std::env::var_os("NORN_SHM_CHILD").map(Into::into)
}

pub async fn child_channel(socket: PathBuf) -> Channel {
    norn_ipc::connect(&Endpoint::Unix(socket)).await.unwrap()
}

pub fn wait_for(child: &mut Child, word: &str) {
    let out = child.stdout.take().unwrap();
    for line in BufReader::new(out).lines() {
        if line.unwrap().contains(word) {
            return;
        }
    }
    panic!("the child never said {word}");
}

/// A call granted a ring, opened by hand; the call stays open while its parts
/// are held.
pub async fn ticket(
    channel: Channel,
) -> (
    SharedMemoryTicket,
    (
        mpsc::Sender<norn_protocol::telemetry::AppendSamplesRequest>,
        Streaming<AppendSamplesResponse>,
    ),
) {
    let plane = super::shm_common::negotiated(&channel)
        .await
        .shared_memory
        .unwrap();
    let (tx, rx) = mpsc::channel(1);
    let mut request = tonic::Request::new(Requests(rx));
    let asked = MetadataValue::from_static(DATA_PLANE_SHARED_MEMORY);
    request
        .metadata_mut()
        .insert(DATA_PLANE_METADATA_KEY, asked);
    let response = TelemetryServiceClient::new(channel)
        .append_samples(request)
        .await
        .unwrap();
    let granted = response.metadata().get(SHARED_MEMORY_SERVICE_METADATA_KEY);
    let granted = granted.unwrap().to_str().unwrap().to_owned();
    let ticket = SharedMemoryTicket::granted(&plane, &granted).unwrap();
    (ticket, (tx, response.into_inner()))
}

/// A publisher on `ticket`'s ring, with nothing between the test and iceoryx2.
pub struct Hostile {
    pub publisher: Publisher<Flavour, [u8], ()>,
    _service: PortFactory<Flavour, [u8], ()>,
    _node: Node<Flavour>,
}

pub fn hostile(ticket: &SharedMemoryTicket) -> Hostile {
    let mut config = Config::default();
    let root = ticket.root_directory.to_string_lossy();
    config
        .global
        .set_root_path(&Path::new(root.as_bytes()).unwrap());
    config.global.prefix = FileName::new(ticket.name_prefix.as_bytes()).unwrap();
    let node = NodeBuilder::new()
        .config(&config)
        .signal_handling_mode(SignalHandlingMode::Disabled)
        .create::<Flavour>()
        .unwrap();
    let name = ServiceName::new(&ticket.service).unwrap();
    let service = node
        .service_builder(&name)
        .publish_subscribe::<[u8]>()
        .open()
        .unwrap();
    let publisher = service
        .publisher_builder()
        .initial_max_slice_len(ticket.max_frame_bytes)
        .create()
        .unwrap();
    Hostile {
        publisher,
        _service: service,
        _node: node,
    }
}
