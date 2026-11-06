//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `migrate` and the daemon's store version check: the real binary, and a real daemon.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

#[path = "offline_common/mod.rs"]
mod common;

use common::{Scratch, append, config, count, declare, endpoint, ok, refused, serving, stop};
use norn_server::Daemon;

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_refuses_a_newer_store_and_migrate_identity_marks_an_old_one() {
    let dir = Scratch::new("ver");
    let (e, data) = (endpoint(&dir.0), dir.0.join("data"));
    let daemon = serving(&e, &data).await;
    declare(&e);
    append(&e, 0, 3);
    stop(daemon).await;
    assert!(
        data.join("FORMAT").exists(),
        "a new database records its version"
    );
    // As 1.0.0-rc.1 left it: no marker. It opens, and gets one.
    std::fs::remove_file(data.join("FORMAT")).unwrap();
    let dry = ok(
        &e,
        &[
            "migrate",
            "--data-dir",
            data.to_str().unwrap(),
            "--to",
            "1",
            "--dry-run",
        ],
    );
    assert!(dry.starts_with("would migrate"), "{dry}");
    assert!(!data.join("FORMAT").exists(), "a dry run changed something");
    let said = ok(
        &e,
        &["migrate", "--data-dir", data.to_str().unwrap(), "--to", "1"],
    );
    assert!(
        said.contains("store_version=1->1") && said.contains("damaged_regions=0"),
        "{said}"
    );
    assert!(data.join("FORMAT").exists());
    let none = refused(
        &e,
        &["migrate", "--data-dir", data.to_str().unwrap(), "--to", "9"],
    );
    assert!(none.contains("no migration leads"), "{none}");
    std::fs::remove_file(data.join("FORMAT")).unwrap();
    let daemon = serving(&e, &data).await;
    assert_eq!(count(&e), 3);
    stop(daemon).await;
    assert!(data.join("FORMAT").exists());
    std::fs::write(data.join("FORMAT"), norn_database::encode_marker(99)).unwrap();
    let start = Daemon::bind(config(&e, &data))
        .await
        .err()
        .unwrap()
        .to_string();
    assert!(start.contains("newer Norn"), "{start}");
    assert_eq!(
        std::fs::read(data.join("FORMAT")).unwrap(),
        norn_database::encode_marker(99)
    );
}
