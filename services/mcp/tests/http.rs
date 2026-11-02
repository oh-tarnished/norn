//! The HTTP transport's guards, against the running binary.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::process::Command;

use common::{Running, http_server, temp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const BODY: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;

/// POSTs an `initialize` as a browser would send it, and returns the status line.
async fn initialize(address: &str, path: &str, host: &str, origin: Option<&str>) -> String {
    let mut stream = TcpStream::connect(address).await.unwrap();
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    let request = format!(
        "POST {path} HTTP/1.1\r\nHost: {host}\r\n{origin}Content-Type: application/json\r\n\
         Accept: application/json, text/event-stream\r\n\
         Content-Length: {}\r\n\r\n{BODY}",
        BODY.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut reply = Vec::new();
    let mut buffer = [0; 256];
    // To the end of the status line; a 200 is a stream that stays open.
    while !reply.windows(2).any(|w| w == b"\r\n") {
        match stream.read(&mut buffer).await {
            Ok(0) => break,
            Ok(n) => reply.extend_from_slice(&buffer[..n]),
            Err(e) => panic!("{host}: {e} after {reply:?}"),
        }
    }
    let reply = String::from_utf8_lossy(&reply);
    reply.lines().next().unwrap_or_default().to_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_host_outside_the_list_is_refused_and_a_named_one_answered() {
    let daemon = Running::start(temp("host")).await;
    let (_child, address) =
        http_server(&daemon.endpoint, &["--mcp-host", "norn.example.org"]).await;
    let port = address.rsplit(':').next().unwrap();
    for host in [
        "localhost",
        "127.0.0.1",
        "norn.example.org",
        &format!("localhost:{port}"),
    ] {
        let reply = initialize(&address, "/mcp", host, None).await;
        assert!(reply.contains(" 200"), "{host}: {reply}");
    }
    for host in [
        "evil.example.net",
        "evil.example.net:80",
        "localhost.evil.example.net",
    ] {
        let reply = initialize(&address, "/mcp", host, None).await;
        assert!(reply.contains(" 403"), "{host}: {reply}");
    }
    daemon.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_web_page_on_another_origin_is_refused_even_with_a_loopback_host() {
    let daemon = Running::start(temp("origin")).await;
    let (_child, address) = http_server(&daemon.endpoint, &[]).await;
    let host = address.clone();
    let reply = initialize(&address, "/mcp", &host, Some("https://evil.example.net")).await;
    assert!(reply.contains(" 403"), "{reply}");
    let reply = initialize(&address, "/mcp", &host, Some("http://localhost:3000")).await;
    assert!(reply.contains(" 200"), "{reply}");
    daemon.finish().await;
}

#[tokio::test(flavor = "multi_thread")]
async fn only_the_mcp_path_is_served() {
    let daemon = Running::start(temp("path")).await;
    let (_child, address) = http_server(&daemon.endpoint, &[]).await;
    let reply = initialize(&address, "/other", "localhost", None).await;
    assert!(reply.contains(" 404"), "{reply}");
    daemon.finish().await;
}

#[test]
fn a_non_loopback_address_is_refused_at_the_command_line() {
    let run = Command::new(env!("CARGO_BIN_EXE_norn-mcp"))
        .args(["--mcp", "http:0.0.0.0:0"])
        .output()
        .unwrap();
    assert_eq!(run.status.code(), Some(2));
    let stderr = String::from_utf8(run.stderr).unwrap();
    assert!(stderr.contains("loopback"), "{stderr}");
}

#[test]
fn help_names_every_option() {
    let run = Command::new(env!("CARGO_BIN_EXE_norn-mcp"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(run.status.success());
    let text = String::from_utf8(run.stdout).unwrap();
    for option in ["--endpoint", "--mcp ", "--mcp-host", "--timeout"] {
        assert!(text.contains(option), "{option}");
    }
}
