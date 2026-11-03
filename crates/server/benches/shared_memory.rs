//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The socket against shared memory, at 1k, 10k and 100k samples a second:
//! docs/reference/shared-memory.md's numbers.
//!
//! `cargo bench -p norn-server --bench shared_memory`. Starts the real `norn`
//! with `[shared_memory] enabled = true` on a Unix socket, creates 16,000
//! signals, and drives each load open-loop twice through the same client code,
//! `norn_ipc::AppendCall`: once over the socket, once over a ring. A row whose
//! plane is not `SharedMemory` was not granted one, and says so. `cargo test
//! --benches` runs a tiny smoke scale (cargo passes `--bench` only to a real
//! bench run). Unix only: a Windows daemon offers no rings.
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

#[cfg(unix)]
#[path = "shared_memory/drive.rs"]
mod drive;
#[cfg(unix)]
#[path = "append/driver.rs"]
#[allow(dead_code)]
mod driver;
#[cfg(unix)]
#[path = "append/setup.rs"]
mod setup;

#[cfg(not(unix))]
fn main() {}

#[cfg(unix)]
#[tokio::main]
async fn main() {
    use std::process::{Command, Stdio};
    use std::sync::Arc;
    use std::time::Duration;

    use drive::{Load, run};
    use norn_ipc::AppendOptions;
    use norn_shared_memory::SharedMemory;

    const LOADS: [Load; 3] = [
        Load {
            rate: 1_000,
            batch: 100,
            secs: 10,
        },
        Load {
            rate: 10_000,
            batch: 1_000,
            secs: 10,
        },
        Load {
            rate: 100_000,
            batch: 2_000,
            secs: 10,
        },
    ];
    let root = std::env::temp_dir().join(format!("norn-shm-bench-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    let file = root.join("norn.toml");
    std::fs::write(&file, "[shared_memory]\nenabled = true\n").unwrap();
    let socket = root.join("ipc/n.sock");
    let mut daemon = Command::new(env!("CARGO_BIN_EXE_norn"))
        .arg("--config")
        .arg(&file)
        .arg("--data-dir")
        .arg(root.join("data"))
        .arg("--listen")
        .arg(format!("unix:{}", socket.display()))
        .args(["--log-level", "warn"])
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let channel = driver::connect(&socket).await;
    let smoke = !std::env::args().any(|a| a == "--bench");
    let names = setup::seed(&channel, if smoke { 20 } else { 16_000 }).await;
    let planes = [
        AppendOptions::default(),
        AppendOptions::default().shared_memory(Arc::new(SharedMemory)),
    ];
    println!("| Offered/s | Plane | Achieved/s | Ack p50 | Ack p99 | Rejected | Peak RSS |");
    println!("| ---: | --- | ---: | ---: | ---: | ---: | ---: |");
    let mut base = 0;
    for mut load in LOADS {
        load.secs = if smoke { 1 } else { load.secs };
        for options in &planes {
            let (rss, stop) = setup::watch_rss(daemon.id());
            let report = run(&channel, options, &names, load, base).await;
            stop.cancel();
            base += i64::try_from(load.rate * load.secs).unwrap();
            let peak = rss.await.unwrap();
            println!(
                "| {} | {:?} | {:.0} | {:.2?} | {:.2?} | {} | {} MiB |",
                load.rate,
                report.plane,
                report.rate,
                report.p50,
                report.p99,
                report.rejected,
                peak / 1024,
            );
            tokio::time::sleep(Duration::from_millis(if smoke { 10 } else { 2_000 })).await;
        }
    }
    daemon.kill().unwrap();
    daemon.wait().unwrap();
    let _ = std::fs::remove_dir_all(&root);
}
