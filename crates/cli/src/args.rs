//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The `nornctl` command line, hand-parsed (SPEC §39: a few flags and commands
//! do not earn a parser dependency).

use std::ffi::OsString;
use std::time::Duration;

use norn_ipc::Endpoint;

mod usage;

pub use usage::USAGE;

/// The default bound on connecting and on each call.
pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(5);

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// Where the daemon listens.
    pub endpoint: Endpoint,
    /// The bound on connecting and on each call.
    pub timeout: Duration,
    /// What to do.
    pub command: Command,
}

/// A `nornctl` command.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Command {
    /// Print [`USAGE`].
    Help,
    /// Run `norn rekey` with these arguments; needs no daemon.
    Rekey(Vec<String>),
    /// Health and the `Server` resource.
    Status,
    /// Protocol negotiation.
    Version,
    /// A resource command: its words, for [`crate::resources::run`].
    Resource(Vec<String>),
    /// A command that works on a stopped daemon's files, with no daemon to connect
    /// to: its words, for [`crate::offline::run`].
    Offline(Vec<String>),
}

/// Parses the arguments after the program name.
///
/// # Errors
///
/// A message for the user: an unknown flag or command, a missing value, a
/// malformed endpoint or timeout, or no command at all.
pub fn parse<I: IntoIterator<Item = OsString>>(args: I) -> Result<Args, String> {
    let mut endpoint = None;
    let mut timeout = DEFAULT_TIMEOUT;
    let mut command = None;
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        let arg = arg
            .into_string()
            .map_err(|a| format!("argument is not UTF-8: {}", a.display()))?;
        let mut value = |flag: &str| {
            args.next()
                .and_then(|v| v.into_string().ok())
                .ok_or_else(|| format!("{flag} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => command = Some(Command::Help),
            "--endpoint" => {
                let text = value("--endpoint")?;
                endpoint = Some(text.parse().map_err(|e| format!("--endpoint: {e}"))?);
            }
            "--timeout" => {
                let secs: f64 = value("--timeout")?
                    .parse()
                    .map_err(|_| "--timeout: not a number of seconds".to_owned())?;
                timeout = Duration::try_from_secs_f64(secs)
                    .ok()
                    .filter(|d| !d.is_zero())
                    .ok_or_else(|| "--timeout: must be positive".to_owned())?;
            }
            "rekey" if command.is_none() => command = Some(Command::Rekey(Vec::new())),
            _ if matches!(command, Some(Command::Rekey(_))) => {
                if let Some(Command::Rekey(words)) = &mut command {
                    words.push(arg);
                }
            }
            "status" if command.is_none() => command = Some(Command::Status),
            "version" if command.is_none() => command = Some(Command::Version),
            "streams" | "entities" | "signals" | "append" | "query" | "state" | "replay"
            | "subscribe" | "export" | "import" | "compact" | "verify" | "retention"
            | "inspect" | "stats" | "benchmark" | "backup"
                if command.is_none() =>
            {
                command = Some(Command::Resource(vec![arg]));
            }
            "restore" | "migrate" if command.is_none() => {
                command = Some(Command::Offline(vec![arg]));
            }
            _ if matches!(command, Some(Command::Resource(_) | Command::Offline(_))) => {
                if let Some(Command::Resource(words) | Command::Offline(words)) = &mut command {
                    words.push(arg);
                }
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    let command = command.ok_or_else(|| "no command given".to_owned())?;
    Ok(Args {
        endpoint: endpoint.unwrap_or_else(Endpoint::platform_default),
        timeout,
        command,
    })
}
