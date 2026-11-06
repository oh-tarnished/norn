//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A small real database on a temporary directory.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_core::{Config, StreamId};
use norn_metadata::Registry;
use norn_storage::{Chunk, StorageOptions, StreamWriter};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-db-{}-{n}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub const SEGMENT: &str = "streams/0000000000000001/0000000000000000-g0000.nseg";

/// A stopped database with one catalogue and one small sealed segment.
pub fn database(root: &Path) {
    let config = Config::default();
    let dir = root.join("metadata");
    let (registry, _) = Registry::open(&dir, config.limits(), config.metadata()).unwrap();
    drop(registry);
    let stream = root.join("streams/0000000000000001");
    let opts = StorageOptions::from_config(&config);
    let mut writer = StreamWriter::open(&stream, StreamId::new(1), opts).unwrap();
    let mut chunk = Chunk::new(norn_core::SignalId::new(1), norn_core::ValueKind::F64).unwrap();
    for n in 0..20u64 {
        let at = norn_core::Timestamp::from_nanos(i64::try_from(n).unwrap());
        chunk
            .push(&norn_core::Sample::new(
                at,
                norn_core::SignalId::new(1),
                norn_core::Value::F64(n as f64),
                norn_core::Quality::GOOD,
                norn_core::Sequence::new(n),
            ))
            .unwrap();
    }
    writer
        .append_encoded(&chunk.encode(&opts).unwrap())
        .unwrap();
    writer.close().unwrap();
}
