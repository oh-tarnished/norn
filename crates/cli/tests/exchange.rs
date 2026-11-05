//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl export` and `import`, the real binary against a real daemon.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use support::Running;

/// A plant: two signals, metadata created by hand, and some samples.
fn fill(daemon: &Running) {
    daemon.ok(&["streams", "create", "plant"]);
    daemon.ok(&["entities", "create", "dev-01"]);
    let create = ["signals", "create", "dev-01"];
    daemon.ok(&[&create[..], &["temp", "plant", "double"]].concat());
    daemon.ok(&[&create[..], &["fan", "plant", "bool"]].concat());
    for (i, value) in ["NaN", "inf", "-0", "21.5"].iter().enumerate() {
        let at = format!("2026-11-02T08:00:0{i}Z");
        let name = "entities/dev-01/signals/temp";
        daemon.ok(&[
            "append",
            "plant",
            name,
            value,
            "--ack",
            "persisted",
            "--at",
            &at,
        ]);
    }
    let name = "entities/dev-01/signals/fan";
    let at = "2026-11-02T08:00:00Z";
    daemon.ok(&[
        "append",
        "plant",
        name,
        "true",
        "--ack",
        "persisted",
        "--at",
        at,
    ]);
}

fn export_import_repeat(format: &str) {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let first = Running::start().await;
        fill(&first);
        let path = first.dir.0.join(format!("plant.{format}"));
        let path = path.to_str().unwrap();
        let text = first.ok(&["export", "all", "--format", format, "--output", path]);
        assert!(text.contains("samples=5 signals=2"), "{text}");
        let again = first.nornctl(&["export", "all", "--format", format, "--output", path]);
        assert!(!again.status.success(), "export must not overwrite");

        let second = Running::start().await;
        let text = second.ok(&["import", path, "--ack", "persisted"]);
        assert!(text.contains("read=5 sent=5 present=0"), "{text}");
        assert!(text.contains("created=4"), "{text}");
        let want = first.ok(&["query", "all"]);
        assert_eq!(second.ok(&["query", "all"]), want);

        let text = second.ok(&["import", path, "--ack", "persisted"]);
        assert!(text.contains("read=5 sent=0 present=5"), "{text}");
        assert!(text.contains("created=0"), "{text}");
        assert_eq!(second.ok(&["query", "all"]), want);
        assert_eq!(
            second.ok(&["streams"]),
            "streams/plant samples=5 buffered=5\n"
        );
        second.stop().await;
        first.stop().await;
    });
}

#[test]
fn csv_export_imports_into_a_fresh_daemon_and_repeating_adds_nothing() {
    export_import_repeat("csv");
}

#[test]
fn parquet_export_imports_into_a_fresh_daemon_and_repeating_adds_nothing() {
    export_import_repeat("parquet");
}

#[test]
fn arrow_export_imports_into_a_fresh_daemon_and_repeating_adds_nothing() {
    export_import_repeat("arrow");
}

#[test]
fn mcap_export_imports_into_a_fresh_daemon_and_repeating_adds_nothing() {
    export_import_repeat("mcap");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_file_contradicting_a_signals_type_is_refused_and_names_it() {
    let daemon = Running::start().await;
    fill(&daemon);
    let path = daemon.dir.0.join("hand.csv");
    let text = "time,signal,stream,type,value,severity,substatus,sequence\n\
        2026-11-02T09:00:00Z,entities/dev-01/signals/temp,streams/plant,int32,7,GOOD,,0\n";
    std::fs::write(&path, text).unwrap();
    let output = daemon.nornctl(&["import", path.to_str().unwrap()]);
    assert!(!output.status.success());
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("entities/dev-01/signals/temp"), "{error}");
    assert!(error.contains("conflict"), "{error}");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_truncated_export_stops_with_the_row_and_keeps_what_came_before() {
    let daemon = Running::start().await;
    fill(&daemon);
    let path = daemon.dir.0.join("cut.csv");
    let path = path.to_str().unwrap();
    daemon.ok(&["export", "all", "--format", "csv", "--output", path]);
    let text = std::fs::read_to_string(path).unwrap();
    let keep = text
        .rfind('\n')
        .and_then(|end| text[..end].rfind('\n'))
        .unwrap();
    std::fs::write(path, &text[..keep - 4]).unwrap();
    let output = daemon.nornctl(&["import", path]);
    assert_eq!(output.status.code(), Some(1));
    let error = String::from_utf8(output.stderr).unwrap();
    assert!(error.contains("csv row 5: truncated"), "{error}");
    daemon.stop().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_export_needing_more_than_it_has_leaves_no_partial_file() {
    let daemon = Running::start().await;
    let path = daemon.dir.0.join("none.csv");
    let path = path.to_str().unwrap();
    let output = daemon.nornctl(&[
        "export",
        "entities/missing/signals/x/y",
        "--format",
        "csv",
        "--output",
        path,
    ]);
    assert!(!output.status.success());
    assert!(!std::path::Path::new(path).exists());
    daemon.stop().await;
}
