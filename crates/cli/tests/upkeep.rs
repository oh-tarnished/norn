//! `compact`, `verify` and `retention`, the real binary against a real daemon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::process::{Command, Output};

use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;

fn nornctl(endpoint: &Endpoint, words: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "5"])
        .args(words)
        .output()
        .unwrap()
}

fn ok(endpoint: &Endpoint, words: &[&str]) -> String {
    let out = nornctl(endpoint, words);
    let text = String::from_utf8_lossy(&out.stdout).into_owned();
    let error = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{words:?}: {text} {error}");
    text
}

async fn serving(
    endpoint: &Endpoint,
    data: &Path,
) -> (CancellationToken, tokio::task::JoinHandle<()>) {
    let config = DaemonConfig::default()
        .listen(endpoint.clone())
        .data_dir(data.to_path_buf());
    let daemon = Daemon::bind(config).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let stop = cancel.clone();
    let task = tokio::spawn(async move {
        daemon.run(stop).await.unwrap();
    });
    health.reached(Health::Serving).await;
    (cancel, task)
}

/// A Unix socket in `dir`; on Windows, where tokio has none, a named pipe.
#[cfg(unix)]
fn endpoint(dir: &Path) -> Endpoint {
    Endpoint::Unix(dir.join("n.sock"))
}

#[cfg(windows)]
fn endpoint(_: &Path) -> Endpoint {
    let name = format!(r"pipe:\\.\pipe\norn-upkeep-{}", std::process::id());
    name.parse().unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compact_verify_and_a_dry_run_then_a_real_retention_pass_work_from_the_command_line() {
    let dir = std::env::temp_dir().join(format!("nctlu{}", std::process::id()));
    let endpoint = endpoint(&dir);
    let data = dir.join("data");
    let (cancel, task) = serving(&endpoint, &data).await;
    ok(&endpoint, &["streams", "create", "plant"]);
    ok(&endpoint, &["entities", "create", "ahu"]);
    ok(
        &endpoint,
        &["signals", "create", "ahu", "t", "plant", "double"],
    );
    cancel.cancel();
    task.await.unwrap();
    for at in ["1000", "100000", "200000"] {
        let (cancel, task) = serving(&endpoint, &data).await;
        let signal = "entities/ahu/signals/t";
        ok(&endpoint, &["append", "plant", signal, "21.5", "--at", at]);
        cancel.cancel();
        task.await.unwrap();
    }
    let (cancel, task) = serving(&endpoint, &data).await;
    let verified = ok(&endpoint, &["verify", "plant"]);
    assert!(
        verified.contains("segments=3 chunks=3 samples=3"),
        "{verified}"
    );
    assert!(verified.contains("damaged_chunks=0"), "{verified}");
    assert!(ok(&endpoint, &["retention", "get", "plant"]).contains("forever"));
    let set = ok(&endpoint, &["retention", "set", "plant", "1d"]);
    assert!(set.contains("max_age=86400s"), "{set}");
    let dry = ok(&endpoint, &["retention", "apply", "plant", "--dry-run"]);
    assert!(dry.contains("would remove segments=2"), "{dry}");
    assert!(ok(&endpoint, &["streams"]).contains("samples=3"));
    let done = ok(&endpoint, &["retention", "apply", "plant"]);
    assert!(done.contains("removed segments=2"), "{done}");
    assert!(ok(&endpoint, &["streams"]).contains("samples=1"));
    assert!(ok(&endpoint, &["status"]).contains("retention.segments: 2"));
    let compacted = ok(&endpoint, &["compact", "plant"]);
    assert!(compacted.contains("streams/plant chunks="), "{compacted}");
    let months = ok(&endpoint, &["retention", "set", "plant", "6mo"]);
    assert!(months.contains("max_months=6"), "{months}");
    let bad = nornctl(&endpoint, &["compact"]);
    assert!(!bad.status.success());
    cancel.cancel();
    task.await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
