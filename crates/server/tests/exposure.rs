//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Regression tests of docs/security/review-2026-11.md: who a real daemon lets in.
#![cfg(unix)]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "mcp_common/mod.rs"]
mod mcp_common;

use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

use mcp_common::{Norn, temp};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"t","version":"1"}}}"#;

/// The status code `/mcp` answers an initialize with, with this `Origin`.
async fn status(port: u16, origin: Option<&str>) -> String {
    let mut tcp = TcpStream::connect(("127.0.0.1", port)).await.unwrap();
    let origin = origin.map_or(String::new(), |o| format!("Origin: {o}\r\n"));
    let request = format!(
        "POST /mcp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n{origin}\
         Content-Type: application/json\r\nAccept: application/json, text/event-stream\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{INITIALIZE}",
        INITIALIZE.len()
    );
    tcp.write_all(request.as_bytes()).await.unwrap();
    let mut head = [0u8; 12];
    tcp.read_exact(&mut head).await.unwrap();
    String::from_utf8_lossy(&head[9..12]).into_owned()
}

#[tokio::test(flavor = "multi_thread")]
async fn a_web_page_cannot_call_mcp_on_loopback_and_an_agent_still_can() {
    let free = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = free.local_addr().unwrap().port();
    drop(free);
    let flags = ["--mcp".to_owned(), format!("http:127.0.0.1:{port}")];
    let norn = Norn::start(temp("origin"), &flags).await;
    let mut up = false;
    for _ in 0..100 {
        if TcpStream::connect(("127.0.0.1", port)).await.is_ok() {
            up = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    assert!(up, "MCP never listened");
    assert_eq!(status(port, Some("https://evil.example")).await, "403");
    assert_eq!(status(port, Some("http://localhost:3000")).await, "200");
    assert_eq!(status(port, None).await, "200");
    norn.finish().await;
}

#[test]
fn an_mcp_socket_in_a_directory_others_can_enter_stops_the_start() {
    let root = temp("open");
    let open = root.join("open");
    std::fs::create_dir(&open).unwrap();
    std::fs::set_permissions(&open, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::create_dir(root.join("ipc")).unwrap();
    std::fs::set_permissions(root.join("ipc"), std::fs::Permissions::from_mode(0o700)).unwrap();
    let run = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", root.join("ipc/n.sock").display()))
        .arg("--mcp")
        .arg(format!("unix:{}", open.join("m.sock").display()))
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(!run.status.success());
    let log = String::from_utf8_lossy(&run.stderr);
    assert!(log.contains("owner-only"), "{log}");
    assert!(!root.join("data/metadata").exists(), "before the database");
    std::fs::remove_dir_all(&root).unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn the_database_directory_is_closed_to_other_users_new_or_not() {
    let root = temp("mode");
    let norn = Norn::start(root.clone(), &[]).await;
    let mode = || {
        std::fs::metadata(root.join("data"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777
    };
    assert_eq!(mode(), 0o700, "created owner-only");
    let root2 = temp("mode2");
    std::fs::create_dir(root2.join("data")).unwrap();
    std::fs::set_permissions(root2.join("data"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let second = Norn::start(root2.clone(), &[]).await;
    let meta = std::fs::metadata(root2.join("data")).unwrap();
    assert_eq!(meta.permissions().mode() & 0o777, 0o700, "tightened");
    second.finish().await;
    norn.finish().await;
}
