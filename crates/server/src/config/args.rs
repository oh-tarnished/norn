//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The `norn` command line. Hand-parsed: seven flags do not earn a parser
//! dependency (SPEC §39).

use std::ffi::OsString;
use std::path::PathBuf;

use super::{ConfigError, DaemonConfig};

/// The `norn --help` text.
pub const USAGE: &str = "\
usage: norn [rekey] [OPTIONS]

Runs the Norn daemon in the foreground until SIGINT or SIGTERM (Ctrl-C on
Windows), then drains in-flight calls and exits. `norn rekey` instead rewrites every
stored segment under the current encryption key, with the daemon stopped, and exits.

options:
  --config PATH        read settings from a TOML file (docs/guides/configuration.md)
  --listen ENDPOINT    serve on ENDPOINT, such as unix:/run/norn/norn.sock,
                       pipe:\\\\.\\pipe\\norn or tcp:127.0.0.1:7171
                       (default: the platform's per-user endpoint)
  --data-dir PATH      keep the database in PATH (default: per-user,
                       docs/guides/configuration.md)
  --mcp TRANSPORT      also serve MCP: stdio, http:127.0.0.1:PORT, unix:PATH or
                       pipe:NAME. Repeatable (docs/reference/mcp.md)
  --log-level LEVEL    error, warn, info (default), debug or trace
  --log-format FORMAT  text (default) or json
  -h, --help           print this text
  -V, --version        print the daemon and protocol versions

Flags override the file; the file overrides the built-in defaults.";

/// What the command line asked for.
#[derive(Debug)]
#[non_exhaustive]
pub enum Invocation {
    /// Run the daemon with this configuration.
    Run(Box<DaemonConfig>),
    /// Rewrite the database under the current key with this configuration.
    Rekey(Box<DaemonConfig>),
    /// Print [`USAGE`] and exit successfully.
    Help,
    /// Print the version and exit successfully.
    Version,
}

/// Parses the arguments after the program name, loading `--config` (blocking
/// file I/O) and applying the other flags over it.
///
/// # Errors
///
/// [`ConfigError::Usage`] for an unknown flag, a missing value or a value
/// that is not UTF-8; otherwise any error of loading or validating.
pub fn parse_args<I>(args: I) -> Result<Invocation, ConfigError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut file: Option<PathBuf> = None;
    let mut listen = None;
    let mut data_dir = None;
    let mut level = None;
    let mut format = None;
    let mut mcp = Vec::new();
    let mut rekey = false;
    let mut args = args.into_iter().peekable();
    if args.peek().is_some_and(|first| first == "rekey") {
        rekey = true;
        args.next();
    }
    while let Some(arg) = args.next() {
        let arg = text(arg)?;
        let mut value = |flag: &str| {
            args.next()
                .ok_or_else(|| ConfigError::Usage(format!("{flag} needs a value")))
        };
        match arg.as_str() {
            "-h" | "--help" => return Ok(Invocation::Help),
            "-V" | "--version" => return Ok(Invocation::Version),
            "--config" => file = Some(PathBuf::from(value("--config")?)),
            "--listen" => listen = Some(text(value("--listen")?)?),
            "--data-dir" => data_dir = Some(PathBuf::from(value("--data-dir")?)),
            "--mcp" => mcp.push(text(value("--mcp")?)?),
            "--log-level" => level = Some(text(value("--log-level")?)?),
            "--log-format" => format = Some(text(value("--log-format")?)?),
            other => return Err(ConfigError::Usage(format!("unknown argument {other}"))),
        }
    }
    let mut config = match file {
        Some(path) => DaemonConfig::load(&path)?,
        None => DaemonConfig::default(),
    };
    if let Some(listen) = listen {
        let endpoint = listen
            .parse()
            .map_err(|e| ConfigError::invalid("--listen", e))?;
        config = config.listen(endpoint);
    }
    if let Some(dir) = data_dir {
        config = config.data_dir(dir);
    }
    for transport in mcp {
        let parsed = transport
            .parse()
            .map_err(|e| ConfigError::invalid("--mcp", e))?;
        config.mcp.enabled = true;
        config.mcp.transports.push(parsed);
    }
    if let Some(level) = level {
        config.log.level = level.parse()?;
    }
    if let Some(format) = format {
        config.log.format = format.parse()?;
    }
    if rekey {
        return Ok(Invocation::Rekey(Box::new(config)));
    }
    Ok(Invocation::Run(Box::new(config)))
}

fn text(arg: OsString) -> Result<String, ConfigError> {
    arg.into_string()
        .map_err(|a| ConfigError::Usage(format!("argument is not UTF-8: {}", a.display())))
}
