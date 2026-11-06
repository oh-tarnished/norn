//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl rekey`: the one command that is not a client of a daemon. It rewrites
//! files with the daemon stopped, so it is the daemon binary's to do, and this
//! crate reaches a database only through the protocol.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// The `norn` beside this `nornctl`, else whichever is on the `PATH`.
fn daemon() -> PathBuf {
    let name = format!("norn{}", std::env::consts::EXE_SUFFIX);
    let beside = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.parent().map(|dir| dir.join(&name)))
        .filter(|path| path.is_file());
    beside.unwrap_or_else(|| PathBuf::from(name))
}

/// Runs `norn rekey words…` to its end, and exits as it does.
#[must_use]
pub fn rekey(words: &[String]) -> ExitCode {
    let path = daemon();
    match Command::new(&path).arg("rekey").args(words).status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("nornctl: rekey: cannot run {}: {e}", path.display());
            ExitCode::FAILURE
        }
    }
}
