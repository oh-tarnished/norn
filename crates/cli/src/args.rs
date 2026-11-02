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
                              send one sample, timed T or now, and wait for its
                              acknowledgement; SIGNAL is a full resource name;
                              LEVEL is buffered, persisted or durable (default)
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
    /// Health and the `Server` resource.
    Status,
    /// Protocol negotiation.
    Version,
    /// A resource command: its words, for [`crate::resources::run`].
    Resource(Vec<String>),
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
            "status" if command.is_none() => command = Some(Command::Status),
            "version" if command.is_none() => command = Some(Command::Version),
            "streams" | "entities" | "signals" | "append" | "query" | "state" | "replay"
            | "subscribe"
                if command.is_none() =>
            {
                command = Some(Command::Resource(vec![arg]));
            }
            _ if matches!(command, Some(Command::Resource(_))) => {
                if let Some(Command::Resource(words)) = &mut command {
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
