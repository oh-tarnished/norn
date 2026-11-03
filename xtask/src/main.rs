//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `cargo run -p xtask -- <gate>`: runs one gate, or `gates` for all of them.
//!
//! Exits non-zero when any gate fails, including a gate that examined
//! nothing.

use std::process::ExitCode;

fn main() -> ExitCode {
    let arg = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "gates".to_owned());
    let root = match xtask::scan::repo_root() {
        Ok(root) => root,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    let gates: Vec<&str> = if arg == "gates" {
        xtask::GATES.to_vec()
    } else {
        vec![arg.as_str()]
    };
    let mut failed = false;
    for gate in gates {
        match xtask::run(gate, &root).and_then(|f| f.verdict()) {
            Ok(report) => println!("{report}"),
            Err(report) => {
                eprintln!("{report}");
                failed = true;
            }
        }
    }
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
