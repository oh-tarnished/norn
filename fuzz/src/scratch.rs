//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Scratch files, for decoders whose input is a file.

use std::path::PathBuf;
use std::sync::OnceLock;

/// A directory of this process's own, created once under the system's
/// temporary directory, holding `name`. Each process has its own, so parallel
/// fuzzing jobs never read each other's inputs.
///
/// # Panics
/// When the directory cannot be created: a target that cannot write its input
/// cannot test anything, and must say so rather than pass.
#[must_use]
pub fn scratch(name: &str) -> PathBuf {
    static DIR: OnceLock<PathBuf> = OnceLock::new();
    let dir = DIR.get_or_init(|| {
        let dir = std::env::temp_dir().join(format!("norn-fuzz-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("a scratch directory");
        dir
    });
    dir.join(name)
}
