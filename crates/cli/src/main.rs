//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl`: see the `norn_cli` library for the commands.
//!
//! Exit status: 0 on success, 1 when the daemon is unreachable or refuses,
//! 2 for a malformed command line.

use std::process::ExitCode;

use norn_cli::{Command, USAGE, connect, offline, parse, resources, status, version};

fn main() -> ExitCode {
    let args = match parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("nornctl: {message}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    if args.command == Command::Help {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    if let Command::Offline(words) = &args.command {
        let mut out = std::io::stdout().lock();
        return match offline::run(words, &mut out) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("nornctl: {e}");
                ExitCode::FAILURE
            }
        };
    }
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            eprintln!("nornctl: cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(async {
        let channel = connect(&args.endpoint, args.timeout).await?;
        let mut out = std::io::stdout().lock();
        match &args.command {
            Command::Version => version(channel, &mut out).await,
            Command::Resource(words) => resources::run(channel, words, &mut out).await,
            _ => status(channel, &mut out).await,
        }
    });
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("nornctl: {e}");
            ExitCode::FAILURE
        }
    }
}
