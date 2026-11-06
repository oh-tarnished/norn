//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A maintenance worker on a real write path, to take backups with.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use norn_core::{MaintenanceConfig, StreamId, Value};
use norn_runtime::maintenance::{BackupReport, BackupSpec, Failure, Maintenance, Outcome, Request};
use tokio_util::sync::CancellationToken;

use super::common::Rig;

/// A worker running on the current tokio runtime, until `stop` is dropped by the test.
pub struct Worker {
    pub maintenance: Arc<Maintenance>,
    stop: CancellationToken,
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}

pub fn worker(rig: &Rig) -> Worker {
    let clock: Arc<dyn norn_runtime::Clock> = Arc::new(rig.clock.clone());
    let memory = norn_runtime::Budget::new("chunk_cache_bytes", 16 << 20, Arc::clone(&clock));
    let mut config = MaintenanceConfig::default();
    config.background = false;
    let maintenance = Arc::new(Maintenance::new(
        Arc::clone(&rig.ingest),
        Arc::clone(&rig.registry),
        memory,
        clock,
        config,
    ));
    let stop = CancellationToken::new();
    tokio::spawn(Arc::clone(&maintenance).run(stop.clone()));
    Worker { maintenance, stop }
}

impl Worker {
    pub async fn backup(&self, to: &Path, base: Option<&Path>) -> Result<BackupReport, Failure> {
        let spec = BackupSpec::new(to.into(), base.map(Into::into), "test-backup".to_owned());
        let op = self
            .maintenance
            .submit(StreamId::new(0), Request::Backup(spec))
            .unwrap();
        loop {
            if let Some((_, result)) = op.ending() {
                return result.map(|o| match o {
                    Outcome::BackedUp(r) => r,
                    other => panic!("{other:?}"),
                });
            }
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }
}

/// The `n`s of `samples`, checked to be the values `common::sample` wrote.
pub fn numbers(samples: &[norn_core::Sample]) -> Vec<u64> {
    let mut out: Vec<u64> = samples
        .iter()
        .map(|s| {
            let n = s.sequence.get();
            assert_eq!(s.value, Value::F64(n as f64));
            n
        })
        .collect();
    out.sort_unstable();
    out
}
