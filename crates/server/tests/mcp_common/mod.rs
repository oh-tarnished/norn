//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The real `norn` binary serving MCP, and rmcp's real client speaking to it.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use norn_protocol::admin::GetServerRequest;
use norn_protocol::admin::admin_service_client::AdminServiceClient;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use rmcp::transport::{StreamableHttpClientTransport, UnixSocketHttpClient};
use serde_json::{Value, json};
use tokio::process::{Child, Command};
use tonic::transport::Channel;

/// A real `norn`, on a socket under `root`.
pub struct Norn {
    pub child: Child,
    pub root: PathBuf,
    pub channel: Channel,
}

/// A fresh short directory: a Unix socket's path has a small limit.
pub fn temp(tag: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nm-{tag}-{}-{n}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub fn mcp_socket(root: &Path) -> PathBuf {
    root.join("mcp/m.sock")
}

impl Norn {
    /// Starts `norn` with `flags`, and waits for its gRPC.
    pub async fn start(root: PathBuf, flags: &[String]) -> Self {
        let socket = root.join("ipc/n.sock");
        let mut command = Command::new(env!("CARGO_BIN_EXE_norn"));
        command
            .arg("--data-dir")
            .arg(root.join("data"))
            .arg("--listen")
            .arg(format!("unix:{}", socket.display()))
            .args(flags)
            .args(["--log-level", "error"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let child = command.spawn().unwrap();
        let endpoint = Endpoint::Unix(socket);
        let deadline = Instant::now() + Duration::from_secs(30);
        let channel = loop {
            if let Ok(channel) = norn_ipc::connect(&endpoint).await {
                let get = GetServerRequest {
                    name: "server".into(),
                };
                if AdminServiceClient::new(channel.clone())
                    .get_server(get)
                    .await
                    .is_ok()
                {
                    break channel;
                }
            }
            assert!(Instant::now() < deadline, "the daemon never answered");
            tokio::time::sleep(Duration::from_millis(5)).await;
        };
        Self {
            child,
            root,
            channel,
        }
    }

    /// `norn` with MCP on a Unix socket and on stdio.
    pub async fn both(tag: &str) -> Self {
        let root = temp(tag);
        let unix = format!("unix:{}", mcp_socket(&root).display());
        let flags = ["--mcp".into(), "stdio".into(), "--mcp".into(), unix];
        Self::start(root, &flags).await
    }

    /// A client on its stdin and stdout, as a client that launched it.
    pub async fn stdio(&mut self) -> Client {
        let out = self.child.stdout.take().unwrap();
        let input = self.child.stdin.take().unwrap();
        Client(().serve((out, input)).await.unwrap())
    }

    /// A client on the MCP Unix socket.
    pub async fn unix(&self) -> Client {
        let uri = "http://localhost/mcp";
        let path = mcp_socket(&self.root);
        let http = UnixSocketHttpClient::new(path.to_str().unwrap(), uri);
        let config = StreamableHttpClientTransportConfig::with_uri(uri);
        let transport = StreamableHttpClientTransport::with_client(http, config);
        Client(().serve(transport).await.unwrap())
    }

    pub async fn finish(mut self) {
        let _ = self.child.kill().await;
        let _ = std::fs::remove_dir_all(&self.root);
    }
}

/// A real MCP client.
pub struct Client(pub RunningService<RoleClient, ()>);

impl Client {
    pub async fn tools(&self) -> Vec<Tool> {
        self.0.list_all_tools().await.unwrap()
    }

    /// The tool's JSON, and whether it was an error result.
    pub async fn raw(&self, tool: &'static str, args: Value) -> (Value, bool) {
        let Value::Object(args) = args else {
            panic!("arguments are an object")
        };
        let request = CallToolRequestParams::new(tool).with_arguments(args);
        let result = self.0.call_tool(request).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        let failed = result.is_error == Some(true);
        (serde_json::from_str(&text).unwrap(), failed)
    }

    /// A call that must succeed.
    pub async fn ok(&self, tool: &'static str, args: Value) -> Value {
        let (value, failed) = self.raw(tool, args).await;
        assert!(!failed, "{tool}: {value}");
        value
    }

    /// A call that must fail: its `error`.
    pub async fn fails(&self, tool: &'static str, args: Value) -> Value {
        let (value, failed) = self.raw(tool, args).await;
        assert!(failed, "{tool} succeeded: {value}");
        value["error"].clone()
    }

    /// Every `field` of a paginated read at `size`, following its token.
    pub async fn all(
        &self,
        tool: &'static str,
        mut args: Value,
        field: &str,
        size: i64,
    ) -> Vec<Value> {
        args["page_size"] = json!(size);
        let mut rows = Vec::new();
        loop {
            let page = self.ok(tool, args.clone()).await;
            let got = page[field].as_array().cloned().unwrap_or_default();
            assert!(got.len() as i64 <= size, "{tool} passed its page size");
            rows.extend(got);
            match page["nextPageToken"].as_str() {
                Some(token) if !token.is_empty() => args["page_token"] = json!(token),
                _ => return rows,
            }
        }
    }
}
