//! nornctl state, replay and subscribe, the real binary against a real daemon.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::{Command, Output};

use norn_ipc::Endpoint;
use norn_runtime::Health;
use norn_server::{Daemon, DaemonConfig};
use tokio_util::sync::CancellationToken;

fn nornctl(endpoint: &Endpoint, words: &[&str]) -> String {
    let output: Output = Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &endpoint.to_string(), "--timeout", "5"])
        .args(words)
        .output()
        .unwrap();
    assert!(output.status.success(), "{words:?}: {output:?}");
    String::from_utf8(output.stdout).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn state_replay_and_subscribe_read_history_back() {
    let dir = std::env::temp_dir().join(format!("nctlh{}", std::process::id()));
    let endpoint = Endpoint::Unix(dir.join("n.sock"));
    let config = DaemonConfig::default()
        .listen(endpoint.clone())
        .data_dir(dir.join("data"));
    let daemon = Daemon::bind(config).await.unwrap();
    let mut health = daemon.health();
    let cancel = CancellationToken::new();
    let task = tokio::spawn(daemon.run(cancel.clone()));
    health.reached(Health::Serving).await;
    let e = &endpoint;
    let run = |w: Vec<&'static str>| {
        let e = e.clone();
        tokio::task::spawn_blocking(move || nornctl(&e, &w))
    };
    run(vec!["streams", "create", "plant"]).await.unwrap();
    run(vec!["entities", "create", "ahu"]).await.unwrap();
    run(vec!["signals", "create", "ahu", "t", "plant", "double"])
        .await
        .unwrap();
    for (value, at) in [("20", "1000"), ("21", "1001"), ("22", "1002")] {
        let words = vec!["append", "plant", "entities/ahu/signals/t", value];
        let mut words = words;
        words.extend(["--ack", "persisted", "--at", at]);
        run(words).await.unwrap();
    }
    let state = run(vec!["state", "ahu", "--at", "1001"]).await.unwrap();
    assert!(state.contains("entities/ahu/signals/t 21 GOOD"), "{state}");
    let made = run(vec![
        "replay", "create", "ahu", "--from", "1000", "--to", "1003", "--speed", "max", "--id", "r1",
    ]);
    let made = made.await.unwrap();
    assert!(made.starts_with("replaySessions/r1 PAUSED"), "{made}");
    let seen = run(vec!["subscribe", "replay", "r1", "--resume"])
        .await
        .unwrap();
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 5, "{seen}");
    assert!(
        lines[0].starts_with("#1 ") && lines[0].ends_with("RUNNING"),
        "{seen}"
    );
    assert!(
        lines[3].contains("entities/ahu/signals/t 22 GOOD"),
        "{seen}"
    );
    assert!(lines[4].ends_with("ENDED"), "{seen}");
    let sought = run(vec!["replay", "seek", "r1", "1001"]).await.unwrap();
    assert!(
        sought.contains("PAUSED") && sought.contains("emitted=0"),
        "{sought}"
    );
    let listed = run(vec!["replay", "list"]).await.unwrap();
    assert!(listed.contains("replaySessions/r1"), "{listed}");
    run(vec!["replay", "delete", "r1"]).await.unwrap();
    cancel.cancel();
    task.await.unwrap().unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}
