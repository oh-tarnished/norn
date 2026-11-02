//! The real `norn-mcp` binary, and rmcp's real client speaking to it.

use std::process::Stdio;

use norn_ipc::Endpoint;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, Tool};
use rmcp::service::{RoleClient, RunningService};
use rmcp::transport::StreamableHttpClientTransport;
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

/// A real MCP client talking to a real `norn-mcp`.
pub struct Client {
    pub service: RunningService<RoleClient, ()>,
    pub child: Child,
    /// `127.0.0.1:port`, over HTTP.
    pub address: Option<String>,
}

fn command(endpoint: &Endpoint, extra: &[&str]) -> Command {
    let mut command = Command::new(env!("CARGO_BIN_EXE_norn-mcp"));
    command
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "10"])
        .args(extra)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    command
}

/// `norn-mcp` as a client launches it: a child speaking on its stdin and stdout.
pub async fn stdio(endpoint: &Endpoint) -> Client {
    let mut child = command(endpoint, &[]).spawn().unwrap();
    let pipes = (child.stdout.take().unwrap(), child.stdin.take().unwrap());
    let service = ().serve(pipes).await.unwrap();
    Client {
        service,
        child,
        address: None,
    }
}

/// `norn-mcp` serving HTTP; returns the child and its address. Its stderr is
/// closed after the first line, as a client discarding logs would, so every test
/// over HTTP also proves a warning it cannot write harms nothing.
pub async fn http_server(endpoint: &Endpoint, extra: &[&str]) -> (Child, String) {
    let mut args = vec!["--mcp", "http:127.0.0.1:0"];
    args.extend_from_slice(extra);
    let mut child = command(endpoint, &args).spawn().unwrap();
    let stderr = child.stderr.take().unwrap();
    let mut lines = BufReader::new(stderr).lines();
    let line = lines.next_line().await.unwrap().unwrap();
    let address = line
        .split("http://")
        .nth(1)
        .unwrap()
        .trim_end_matches("/mcp");
    (child, address.to_owned())
}

/// `norn-mcp` over streamable HTTP, as a client reaches a running server.
pub async fn http(endpoint: &Endpoint) -> Client {
    let (child, address) = http_server(endpoint, &[]).await;
    let uri = format!("http://{address}/mcp");
    let service = ().serve(StreamableHttpClientTransport::from_uri(uri)).await.unwrap();
    Client {
        service,
        child,
        address: Some(address),
    }
}

impl Client {
    pub async fn tools(&self) -> Vec<Tool> {
        self.service.list_all_tools().await.unwrap()
    }

    /// The tool's JSON, and whether it was an error result.
    pub async fn raw(&self, tool: &'static str, args: Value) -> (Value, bool) {
        let Value::Object(args) = args else {
            panic!("arguments are an object")
        };
        let request = CallToolRequestParams::new(tool).with_arguments(args);
        let result = self.service.call_tool(request).await.unwrap();
        let text = result
            .content
            .first()
            .unwrap()
            .as_text()
            .unwrap()
            .text
            .clone();
        (
            serde_json::from_str(&text).unwrap(),
            result.is_error == Some(true),
        )
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

    /// Every row of a read, following continuations; asserts each call stayed bounded.
    pub async fn all(&self, tool: &'static str, mut args: Value, limit: u64) -> Vec<Value> {
        args["limit"] = limit.into();
        let mut rows = Vec::new();
        loop {
            let page = self.ok(tool, args.clone()).await;
            let got = page["rows"].as_array().unwrap();
            assert!(got.len() as u64 <= limit, "{tool} passed its limit");
            assert_eq!(page["count"], got.len());
            rows.extend(got.iter().cloned());
            if page["complete"] == true {
                assert!(page["continuation"].is_null());
                return rows;
            }
            args["continuation"] = page["continuation"].clone();
        }
    }
}
