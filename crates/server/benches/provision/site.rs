//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The deployment being provisioned: devices as entities, points as signals, each
//! annotated the way a device adapter annotates them, so requests have a
//! realistic size.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::common::ValueType;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use tonic::transport::Channel;

/// How big a site is: `devices` entities of `points` signals each.
#[derive(Debug, Clone, Copy)]
pub struct Shape {
    pub devices: usize,
    pub points: usize,
}

impl Shape {
    pub fn signals(self) -> usize {
        self.devices * self.points
    }
}

/// A `norn` process on a fresh database, killed and removed on drop.
pub struct Daemon {
    child: Child,
    root: PathBuf,
    pub socket: PathBuf,
}

impl Daemon {
    /// Starts one, raising `max_signals` so the largest site fits twice.
    pub fn start(tag: &str) -> Self {
        let root = std::env::temp_dir().join(format!("nprov-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let config = root.join("norn.toml");
        let text = "[limits]\nmax_signals = 200000\nmax_entities = 200000\n";
        std::fs::write(&config, text).unwrap();
        let socket = root.join("ipc/n.sock");
        let child = Command::new(env!("CARGO_BIN_EXE_norn"))
            .arg("--config")
            .arg(&config)
            .arg("--data-dir")
            .arg(root.join("data"))
            .arg("--listen")
            .arg(format!("unix:{}", socket.display()))
            .args(["--log-level", "warn"])
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap();
        Self {
            child,
            root,
            socket,
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A channel, once the daemon answers. Each call opens its own connection.
pub async fn connect(socket: &Path) -> Channel {
    let endpoint = Endpoint::Unix(socket.to_path_buf());
    loop {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let get = GetServerRequest {
                name: "server".into(),
            };
            let mut admin = AdminServiceClient::new(channel.clone());
            if admin.get_server(get).await.is_ok() {
                return channel;
            }
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

pub async fn create_stream(channel: &Channel) {
    let stream = CreateStreamRequest {
        stream_id: "site".into(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    let mut client = StreamServiceClient::new(channel.clone());
    client.create_stream(stream).await.unwrap();
}

/// Device `d`, as its create request.
pub fn entity(d: usize) -> CreateEntityRequest {
    let annotations = HashMap::from([
        ("protocol".to_owned(), "bacnet".to_owned()),
        ("device_instance".to_owned(), (100_000 + d).to_string()),
        ("network".to_owned(), "2001".to_owned()),
        ("vendor".to_owned(), "Example Controls".to_owned()),
    ]);
    CreateEntityRequest {
        entity_id: format!("device-{d}"),
        entity: Some(Entity {
            display_name: format!("Level {} AHU {d}", d % 40),
            annotations,
            ..Entity::default()
        }),
        validate_only: false,
    }
}

/// Point `p` of device `d`, as its create request.
pub fn signal(d: usize, p: usize) -> CreateSignalRequest {
    let annotations = HashMap::from([
        ("protocol".to_owned(), "bacnet".to_owned()),
        ("device_instance".to_owned(), (100_000 + d).to_string()),
        ("object_type".to_owned(), "analog-input".to_owned()),
        ("object_instance".to_owned(), p.to_string()),
        (
            "object_name".to_owned(),
            format!("L{}.AHU{d}.SAT{p}", d % 40),
        ),
        ("cov".to_owned(), "true".to_owned()),
    ]);
    CreateSignalRequest {
        parent: format!("entities/device-{d}"),
        signal_id: format!("ai-{p}"),
        signal: Some(Signal {
            display_name: format!("Supply air temperature {p}"),
            value_type: ValueType::Double as i32,
            stream: "streams/site".into(),
            unit: "degree_celsius".into(),
            annotations,
            ..Signal::default()
        }),
        validate_only: false,
    }
}
