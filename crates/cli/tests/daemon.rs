//! nornctl, the real binary, against a real daemon on a temporary endpoint.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::{Command, Output};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh Unix socket; on Windows, where tokio has none, a named pipe.
#[cfg(unix)]
fn endpoint() -> Endpoint {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let dir = std::env::temp_dir().join(format!("nctl{}-{n}", std::process::id()));
    Endpoint::Unix(dir.join("n.sock"))
}

#[cfg(windows)]
fn endpoint() -> Endpoint {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let name = format!(r"pipe:\\.\pipe\norn-ctl-{}-{n}", std::process::id());
    name.parse().unwrap()
}

fn nornctl(endpoint: &Endpoint, command: &str) -> Output {
    let words: Vec<&str> = command.split(' ').collect();
    Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "5"])
        .args(words)
        .output()
        .unwrap()
}

/// The example, built beside the test by `cargo test`; absent under a filter
/// that skips examples.
fn example(name: &str) -> Option<std::path::PathBuf> {
    let deps = std::env::current_exe()
        .ok()?
        .parent()?
        .parent()?
        .to_path_buf();
    let name = format!("{name}{}", std::env::consts::EXE_SUFFIX);
    Some(deps.join("examples").join(name)).filter(|p| p.exists())
}

fn stdout(output: &Output) -> String {
    assert!(output.status.success(), "{output:?}");
    String::from_utf8(output.stdout.clone()).unwrap()
}

/// `status`, retried within a bound until health has reached the reporter:
/// the runtime's `Serving` and the health task's report are not one step.
fn status_when_serving(endpoint: &Endpoint) -> String {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        let text = stdout(&nornctl(endpoint, "status"));
        if text.contains("health: SERVING") {
            return text;
        }
        assert!(Instant::now() < deadline, "never SERVING:\n{text}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

// Multi-threaded: the test blocks on nornctl while the daemon serves.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn status_and_version_work_and_cancelling_drains_and_removes_the_socket() {
    let wanted = endpoint();
    let data = std::env::temp_dir().join(format!("nctl-data-{}", std::process::id()));
    let config = DaemonConfig::default()
        .listen(wanted.clone())
        .data_dir(data.clone());
    let daemon = Daemon::bind(config).await.unwrap();
    assert_eq!(daemon.local_endpoint(), &wanted);
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    assert_eq!(health.reached(Health::Serving).await, Health::Serving);

    let status = status_when_serving(&wanted);
    assert!(status.contains("protocol: 1.3"), "{status}");
    assert!(
        status.contains("capabilities: CAPABILITY_APPEND_SAMPLES"),
        "{status}"
    );
    assert!(status.contains("recovery.corruptions: 0"), "{status}");
    assert!(status.contains("limits.max_streams: 1024"), "{status}");

    let version = stdout(&nornctl(&wanted, "version"));
    assert!(version.contains("client_protocol: 1.3"), "{version}");
    assert!(version.contains("agreed_protocol: 1.3"), "{version}");
    assert!(version.contains(norn_server::SERVER_VERSION), "{version}");

    stdout(&nornctl(&wanted, "streams create plant"));
    stdout(&nornctl(&wanted, "entities create ahu-01"));
    stdout(&nornctl(&wanted, "signals create ahu-01 temp plant double"));
    let listed = stdout(&nornctl(&wanted, "streams"));
    assert!(listed.contains("streams/plant"), "{listed}");
    let listed = stdout(&nornctl(&wanted, "entities"));
    assert!(listed.contains("entities/ahu-01"), "{listed}");
    let listed = stdout(&nornctl(&wanted, "signals"));
    assert!(listed.contains("entities/ahu-01/signals/temp"), "{listed}");
    assert!(listed.contains("VALUE_TYPE_DOUBLE"), "{listed}");
    let sent = "append plant entities/ahu-01/signals/temp 21.5 --ack durable";
    let appended = stdout(&nornctl(&wanted, sent));
    assert!(
        appended.contains("ACKNOWLEDGEMENT_LEVEL_DURABLE"),
        "{appended}"
    );
    assert!(
        appended.contains("BACKPRESSURE_OUTCOME_ACCEPT"),
        "{appended}"
    );
    if let Some(client) = example("plain_client") {
        let run = Command::new(client)
            .arg(wanted.to_string())
            .output()
            .unwrap();
        let text = stdout(&run);
        assert!(text.contains("ACKNOWLEDGEMENT_LEVEL_DURABLE"), "{text}");
    }
    let queried = stdout(&nornctl(&wanted, "query entities/ahu-01/signals/temp"));
    assert!(
        queried.contains("entities/ahu-01/signals/temp 21.5 GOOD"),
        "{queried}"
    );
    let counted = stdout(&nornctl(&wanted, "query ahu-01 --agg count"));
    assert!(counted.contains("count=1"), "{counted}");
    if let Some(client) = example("query_client") {
        let run = Command::new(client)
            .args([&wanted.to_string(), "entities/ahu-01", "temp"])
            .output()
            .unwrap();
        assert!(stdout(&run).contains("1 samples"), "{run:?}");
    }
    let wrong = nornctl(&wanted, "append plant entities/ahu-01/signals/none 1");
    assert_eq!(wrong.status.code(), Some(1));

    cancel.cancel();
    let report = task.await.unwrap().unwrap();
    assert_eq!(report.aborted, 0);
    assert_eq!(health.get(), Health::Stopped);
    std::fs::remove_dir_all(&data).unwrap();

    if let Endpoint::Unix(path) = &wanted {
        assert!(!path.exists(), "the socket must be removed");
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }
    let gone = nornctl(&wanted, "status");
    assert_eq!(gone.status.code(), Some(1));
}

#[test]
fn no_daemon_is_a_one_line_failure_and_a_bad_command_is_a_usage_error() {
    let absent = nornctl(&endpoint(), "version");
    assert_eq!(absent.status.code(), Some(1));
    let stderr = String::from_utf8(absent.stderr).unwrap();
    assert!(stderr.contains("cannot reach the daemon"), "{stderr}");

    let bad = nornctl(&endpoint(), "frobnicate");
    assert_eq!(bad.status.code(), Some(2));
}

#[test]
fn parsing_refuses_a_bad_endpoint_and_a_bad_timeout() {
    let parse = |list: &[&str]| norn_cli::parse(list.iter().map(Into::into));
    assert!(parse(&["--endpoint", "tcp:10.0.0.1:1", "status"]).is_err());
    assert!(parse(&["--timeout", "0", "status"]).is_err());
    assert!(parse(&["--timeout", "-1", "status"]).is_err());
    assert!(parse(&[]).is_err());
    let args = parse(&["--timeout", "1.5", "version"]).unwrap();
    assert_eq!(args.timeout, Duration::from_millis(1500));
    assert_eq!(args.command, norn_cli::Command::Version);
}
