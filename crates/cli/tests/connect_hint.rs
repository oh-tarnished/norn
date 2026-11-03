//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What `nornctl` says when nothing answers on a Unix socket.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::process::Command;

#[test]
fn a_failed_socket_connect_names_the_likely_causes() {
    let dir = std::env::temp_dir().join(format!("nctlc{}", std::process::id()));
    let socket = dir.join("absent").join("n.sock");
    let out = Command::new(env!("CARGO_BIN_EXE_nornctl"))
        .args(["--endpoint", &format!("unix:{}", socket.display())])
        .args(["--timeout", "2", "status"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8(out.stderr).unwrap();
    assert!(stderr.contains("cannot reach the daemon"), "{stderr}");
    assert!(stderr.contains("is the daemon running?"), "{stderr}");
    assert!(stderr.contains("0700"), "{stderr}");
    assert_eq!(stderr.lines().count(), 1, "one line: {stderr}");
    let _ = std::fs::remove_dir_all(&dir);
}
