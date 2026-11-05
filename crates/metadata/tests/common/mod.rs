//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Temporary directories and a seeded registry.
#![allow(dead_code, clippy::unwrap_used)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_core::{Limits, MetadataLimits, Timestamp, ValueKind};
use norn_metadata::{EntitySpec, Registry, SignalSpec, StreamSpec, Write};

static NEXT: AtomicUsize = AtomicUsize::new(0);

pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("norn-meta-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
}

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn open(dir: &Path) -> Registry {
    Registry::open(dir, Limits::default(), MetadataLimits::default())
        .unwrap()
        .0
}

pub fn t(n: i64) -> Timestamp {
    Timestamp::from_nanos(n)
}

pub fn seed(r: &Registry) {
    r.create_stream("plant", StreamSpec::default(), t(1), Write::Commit)
        .unwrap();
    r.create_entity("dev-01", EntitySpec::default(), t(1), Write::Commit)
        .unwrap();
    let spec = SignalSpec::new(ValueKind::F64, "plant".into());
    r.create_signal("dev-01", "temp", spec, t(1), Write::Commit)
        .unwrap();
}
