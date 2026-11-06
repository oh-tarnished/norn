//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The `nornctl` command line, hand-parsed (SPEC §39: a few flags and commands
//! do not earn a parser dependency).

use std::ffi::OsString;
use std::time::Duration;

use norn_ipc::Endpoint;

/// The `nornctl --help` text.
pub const USAGE: &str = "\
usage: nornctl [OPTIONS] COMMAND

commands:
  status    the daemon's health, version, capabilities and limits
  version   this client's protocol version and the daemon's answer to it
  streams [create ID]         list streams, or create one
  entities [create ID]        list entities, or create one
  signals [ENTITY]            list signals, of one entity or all
  signals create ENTITY ID STREAM TYPE
                              create a signal; TYPE is bool, int32, int64,
                              uint32, uint64, float, double, string, bytes, enum
  append STREAM SIGNAL VALUE [--ack LEVEL] [--sequence N] [--at T]
         [--shared-memory]
                              send one sample, timed T or now, and wait for its
                              acknowledgement; SIGNAL is a full resource name;
                              LEVEL is buffered, persisted or durable (default);
                              --shared-memory uses the daemon's shared-memory
                              plane when offered, the socket otherwise
  query TARGET [--from T] [--to T] [--where F] [--select F] [--agg FN]
        [--window D] [--order signal|time] [--limit N] [--resume TOKEN]
                              read samples or aggregates; TARGET is a signal,
                              an entity, or all; T is RFC 3339 UTC, Unix seconds,
                              now or now-D; D is like 500ms, 10s, 5m, 1h or 1d;
                              F is a filter, such as \"value > 20.5\"; FN is
                              latest, first, last, count, min, max, average,
                              sum or rate
  state TARGET [--at T] [--max-age D] [--select F]
                              what each signal held at T (default now); a
                              signal with none, or older than D, is unknown
  replay create TARGET --from T --to T [--speed X|max] [--id ID] [--select F]
  replay list | get|pause|resume|stop|delete NAME | seek NAME T
  replay step NAME N|D | speed NAME X|max
                              a replay session: the clock, its pace, steps
                              of N events or D of virtual time
  subscribe live TARGET [--select F] [--limit N]
  subscribe replay NAME [--resume] [--limit N]
                              print events as they come, numbered; --resume
                              starts the session once subscribed
  export TARGET --format F --output PATH [--from T] [--to T] [--select F]
                              write samples to a new file; F is csv, parquet,
                              arrow or mcap
  import PATH [--format F] [--ack LEVEL] [--batch N] [--shared-memory]
         [--timing]
                              read a file into the daemon, creating what it
                              declares; safe to repeat; F defaults from PATH;
                              --timing prints the rate, as --shared-memory does
  rekey [--config PATH] [--data-dir PATH]
                              stop the daemon first: rewrite stored segments under
                              the current encryption key, by running the norn
                              beside this nornctl
  compact STREAM              merge a stream's sealed segments, and wait
  verify STREAM               read and check every stored chunk, and wait; exits
                              1 if any is damaged
  backup --to DIR [--incremental-from BACKUP] [--id ID]
                              copy the database while the daemon runs, and wait;
                              DIR is empty or new; each stream is cut, so it holds
                              what was admitted before that and nothing after; with
                              --incremental-from, sealed segments BACKUP holds are
                              not copied again, and restoring needs both
  restore --from DIR --data-dir D [--dry-run]
                              offline: write backup DIR into D, new or empty, then
                              check it; run it again after a crash to finish;
                              --dry-run reads every file and writes nothing
  migrate --data-dir D --to VERSION [--dry-run] [--keep-old]
                              offline: move D to a newer store version, checking
                              the result; run it again after a crash to finish;
                              --keep-old keeps each replaced file beside the new
  retention get STREAM        a stream's policy
  retention set STREAM forever|Nd|Nmo|Nb
                              keep everything, or D of time, N calendar months,
                              or N bytes; D is like 30d. Applied at the next pass
  retention apply STREAM [--dry-run]
                              run the policy now; --dry-run removes nothing and
                              lists what would go
  inspect STREAM [SEGMENT] [--basic]
                              how it sits on disk: segments, chunks, codecs,
                              compression ratio, time span; --basic reads only
                              the index, so no codecs and no ratio
  stats [TARGET] [--from T] [--to T]
                              per signal and stream: samples and rate over the
                              window (default the last hour), stored bytes
  benchmark [--stream ID] [--signals N] [--rate N|max] [--duration D]
            [--batch N] [--ack LEVEL] [--queries N] [--allow-non-empty]
                              append then query, printing samples/s and p50/p99
                              latency; writes to its own stream, and refuses
                              one holding samples unless --allow-non-empty

options:
  --endpoint ENDPOINT  the daemon's endpoint, such as unix:/run/norn/norn.sock
                       (default: the platform's per-user endpoint, as norn's)
  --timeout SECONDS    give up on connecting or on any call after this long
                       (default 5)
  -h, --help           print this text";

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
