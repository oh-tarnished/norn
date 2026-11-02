//! The real `norn` binary: signals, draining and its claim on the socket.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use tonic_health::pb::HealthCheckRequest;
use tonic_health::pb::health_check_response::ServingStatus;
use tonic_health::pb::health_client::HealthClient;

static NEXT: AtomicUsize = AtomicUsize::new(0);
const BOUND: Duration = Duration::from_secs(20);

/// A socket path in a fresh directory, short enough for `sun_path`.
fn socket() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir()
        .join(format!("nsig{}-{n}", std::process::id()))
        .join("n.sock")
}

fn start(path: &std::path::Path) -> Child {
    Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--data-dir")
        .arg(path.with_file_name("data"))
        .args(["--log-level", "error", "--listen"])
        .arg(format!("unix:{}", path.display()))
        .stderr(Stdio::null())
        .spawn()
        .unwrap()
}

/// Waits, within [`BOUND`], for the daemon at `path` to report `SERVING`.
async fn wait_serving(path: &std::path::Path) {
    let endpoint = Endpoint::Unix(path.to_path_buf());
    let deadline = Instant::now() + BOUND;
    while Instant::now() < deadline {
        if let Ok(channel) = norn_ipc::connect(&endpoint).await {
            let request = HealthCheckRequest {
                service: String::new(),
            };
            if let Ok(reply) = HealthClient::new(channel).check(request).await
                && reply.into_inner().status == ServingStatus::Serving as i32
            {
                return;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the daemon never reported SERVING");
}

fn signal(child: &Child, name: &str) {
    let sent = Command::new("kill")
        .args([format!("-{name}"), child.id().to_string()])
        .status()
        .unwrap();
    assert!(sent.success());
}

/// Waits, within [`BOUND`], for `child` to exit; kills it and fails if not.
async fn wait_exit(child: &mut Child) -> ExitStatus {
    let deadline = Instant::now() + BOUND;
    while Instant::now() < deadline {
        if let Some(status) = child.try_wait().unwrap() {
            return status;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    child.kill().unwrap();
    panic!("the daemon did not exit after the signal");
}

async fn stops_cleanly_on(name: &str) {
    let path = socket();
    let mut child = start(&path);
    wait_serving(&path).await;
    signal(&child, name);
    let status = wait_exit(&mut child).await;
    assert!(status.success(), "{status:?}");
    assert!(!path.exists(), "the socket must be removed");
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[tokio::test]
async fn sigterm_drains_exits_zero_and_removes_the_socket() {
    stops_cleanly_on("TERM").await;
}

#[tokio::test]
async fn sigint_drains_exits_zero_and_removes_the_socket() {
    stops_cleanly_on("INT").await;
}

#[tokio::test]
async fn a_second_daemon_on_a_live_socket_fails_and_leaves_the_first_serving() {
    let path = socket();
    let mut first = start(&path);
    wait_serving(&path).await;
    let mut second = start(&path);
    let status = wait_exit(&mut second).await;
    assert_eq!(status.code(), Some(1));
    wait_serving(&path).await;
    signal(&first, "TERM");
    assert!(wait_exit(&mut first).await.success());
    std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
}

#[test]
fn a_refused_configuration_exits_two() {
    let status = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--no-such-flag")
        .stderr(Stdio::null())
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(2));
}
