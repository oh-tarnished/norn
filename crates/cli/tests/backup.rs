//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `backup` and `restore`: the real binary, and a real daemon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "offline_common/mod.rs"]
mod common;

use common::{Scratch, append, config, count, declare, endpoint, ok, refused, serving, stop};
use norn_server::Daemon;

#[tokio::test(flavor = "multi_thread")]
async fn a_backup_taken_while_serving_restores_and_verifies_from_the_command_line() {
    let dir = Scratch::new("cycle");
    let (e, data) = (endpoint(&dir.0), dir.0.join("data"));
    let daemon = serving(&e, &data).await;
    declare(&e);
    append(&e, 0, 10);
    let (full, more) = (dir.0.join("full"), dir.0.join("more"));
    let said = ok(
        &e,
        &["backup", "--to", full.to_str().unwrap(), "--id", "first"],
    );
    assert!(said.contains("streams=1"), "{said}");
    append(&e, 10, 15);
    let said = ok(
        &e,
        &[
            "backup",
            "--to",
            more.to_str().unwrap(),
            "--incremental-from",
            full.to_str().unwrap(),
        ],
    );
    assert!(said.contains("copied_files=2"), "{said}");
    let used = refused(&e, &["backup", "--to", full.to_str().unwrap()]);
    assert!(used.contains("is not empty"), "{used}");
    // Not offline: the daemon holds the lock of its own directory.
    let busy = refused(
        &e,
        &["migrate", "--data-dir", data.to_str().unwrap(), "--to", "1"],
    );
    assert!(busy.contains("stop the daemon first"), "{busy}");
    stop(daemon).await;

    let intact = ok(
        &e,
        &["restore", "--from", more.to_str().unwrap(), "--dry-run"],
    );
    assert!(
        intact.contains("backup intact") && intact.contains("chain=2"),
        "{intact}"
    );
    let refused_dir = refused(
        &e,
        &[
            "restore",
            "--from",
            more.to_str().unwrap(),
            "--data-dir",
            data.to_str().unwrap(),
        ],
    );
    assert!(refused_dir.contains("not empty"), "{refused_dir}");
    let restored = dir.0.join("restored");
    ok(
        &e,
        &[
            "restore",
            "--from",
            more.to_str().unwrap(),
            "--data-dir",
            restored.to_str().unwrap(),
        ],
    );
    let early = dir.0.join("early");
    ok(
        &e,
        &[
            "restore",
            "--from",
            full.to_str().unwrap(),
            "--data-dir",
            early.to_str().unwrap(),
        ],
    );

    let daemon = serving(&e, &restored).await;
    assert_eq!(count(&e), 15);
    let verify = ok(&e, &["verify", "plant"]);
    assert!(verify.contains("damaged_chunks=0"), "{verify}");
    assert!(
        verify.contains("segments=2 chunks=2 samples=15"),
        "{verify}"
    );
    stop(daemon).await;
    let daemon = serving(&e, &early).await;
    assert_eq!(count(&e), 10);
    stop(daemon).await;
}

#[tokio::test(flavor = "multi_thread")]
async fn a_corrupted_backup_is_refused_and_the_daemon_will_not_serve_what_it_left() {
    let dir = Scratch::new("bad");
    let (e, data) = (endpoint(&dir.0), dir.0.join("data"));
    let daemon = serving(&e, &data).await;
    declare(&e);
    append(&e, 0, 5);
    let b = dir.0.join("b");
    ok(&e, &["backup", "--to", b.to_str().unwrap()]);
    stop(daemon).await;
    let streams = b.join("streams");
    let stream = std::fs::read_dir(&streams)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let segment = std::fs::read_dir(&stream)
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let mut bytes = std::fs::read(&segment).unwrap();
    let middle = bytes.len() / 2;
    bytes[middle] ^= 0x20;
    std::fs::write(&segment, bytes).unwrap();
    let flagged = refused(&e, &["restore", "--from", b.to_str().unwrap(), "--dry-run"]);
    assert!(flagged.contains("CRC-32C"), "{flagged}");
    let target = dir.0.join("restored");
    let failed = refused(
        &e,
        &[
            "restore",
            "--from",
            b.to_str().unwrap(),
            "--data-dir",
            target.to_str().unwrap(),
        ],
    );
    assert!(failed.contains("CRC-32C"), "{failed}");
    let refused_start = Daemon::bind(config(&e, &target))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(
        refused_start.contains("unfinished nornctl restore"),
        "{refused_start}"
    );
}
