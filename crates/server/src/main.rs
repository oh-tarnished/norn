//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `norn`: the Norn daemon. Everything but process wiring is in the
//! `norn_server` library; see its documentation and docs/guides/configuration.md.
//!
//! Exit status: 0 after a clean shutdown, 1 when the daemon failed, 2 for a
//! command line or configuration it refused.

use std::process::ExitCode;

use norn_server::config::{Invocation, USAGE, parse_args};
use norn_server::{Daemon, DaemonConfig, SERVER_VERSION, logging, shutdown_requested};
use tokio_util::sync::CancellationToken;

fn main() -> ExitCode {
    let (config, rekeying) = match parse_args(std::env::args_os().skip(1)) {
        Ok(Invocation::Run(config)) => (config, false),
        Ok(Invocation::Rekey(config)) => (config, true),
        Ok(Invocation::Help) => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Ok(Invocation::Version) => {
            let p = norn_protocol::PROTOCOL_VERSION;
            println!("norn {SERVER_VERSION} (protocol {}.{})", p.major, p.minor);
            return ExitCode::SUCCESS;
        }
        Ok(_) => return ExitCode::from(2),
        Err(e) => {
            eprintln!("norn: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    logging::init(config.logging());
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!(error = %e, "cannot start the async runtime");
            return ExitCode::FAILURE;
        }
    };
    if rekeying {
        return runtime.block_on(rekey(&config));
    }
    runtime.block_on(serve(*config))
}

/// `norn rekey`. Exit status 1 for anything left under an older key: that key must
/// stay in the key file, and a script must be able to tell.
async fn rekey(config: &DaemonConfig) -> ExitCode {
    let report = match norn_server::rekey(config).await {
        Ok(report) => report,
        Err(e) => {
            eprintln!("norn: rekey: {e}");
            return ExitCode::FAILURE;
        }
    };
    println!(
        "rekeyed rewritten={} current={} left={}",
        report.rewritten,
        report.current,
        report.left.len()
    );
    for (path, why) in &report.left {
        eprintln!(
            "norn: rekey: left under its older key: {}: {why}",
            path.display()
        );
    }
    match report.left.is_empty() {
        true => ExitCode::SUCCESS,
        false => ExitCode::FAILURE,
    }
}

async fn serve(config: DaemonConfig) -> ExitCode {
    let shutdown = match shutdown_requested() {
        Ok(signal) => signal,
        Err(e) => {
            tracing::error!(error = %e, "cannot register for shutdown signals");
            return ExitCode::FAILURE;
        }
    };
    let daemon = match Daemon::bind(config).await {
        Ok(d) => d,
        Err(e) => {
            tracing::error!(error = %e, "cannot start");
            return ExitCode::FAILURE;
        }
    };
    tracing::info!(endpoint = %daemon.local_endpoint(), version = SERVER_VERSION, "listening");
    let cancel = CancellationToken::new();
    let run = daemon.run(cancel.clone());
    tokio::pin!(run);
    let result = tokio::select! {
        result = &mut run => result,
        () = shutdown => {
            cancel.cancel();
            run.await
        }
    };
    match result {
        Ok(report) => {
            tracing::info!(aborted = report.aborted, "stopped");
            ExitCode::SUCCESS
        }
        Err(e) => {
            tracing::error!(error = %e, "failed");
            ExitCode::FAILURE
        }
    }
}
