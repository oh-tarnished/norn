//! The command line.

use std::ffi::OsString;
use std::net::SocketAddr;
use std::time::Duration;

use norn_ipc::Endpoint;

/// The text `--help` prints.
pub const USAGE: &str = "usage: norn-mcp [OPTIONS]

An MCP server over a Norn daemon. Read-only; it never touches storage.

options:
  --endpoint ENDPOINT  the daemon's endpoint, such as unix:/run/norn/norn.sock
                       (default: the platform's per-user endpoint, as norn's)
  --mcp TRANSPORT      stdio, or http:127.0.0.1:8090 for streamable HTTP at /mcp
                       on a loopback address (default: stdio)
  --mcp-host NAME      with http: also answer to this Host, such as a tunnel's.
                       Repeatable. Loopback names are always answered
  --timeout SECONDS    bound on each tool call (default 30)
  -h, --help           print this text
  -V, --version        print the version";

/// How a client reaches this server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Transport {
    /// One client, on this process's stdin and stdout.
    Stdio,
    /// Streamable HTTP at `/mcp` on a loopback address.
    Http(SocketAddr),
}

/// A parsed command line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Args {
    /// The daemon's endpoint.
    pub endpoint: Endpoint,
    /// How clients reach this server.
    pub transport: Transport,
    /// Host names answered beyond loopback's.
    pub hosts: Vec<String>,
    /// The bound on each tool call.
    pub timeout: Duration,
    /// Print [`USAGE`] and exit.
    pub help: bool,
    /// Print the version and exit.
    pub version: bool,
}

fn transport(word: &str) -> Result<Transport, String> {
    if word == "stdio" {
        return Ok(Transport::Stdio);
    }
    let bad = || format!("--mcp is stdio or http:ADDRESS:PORT, not {word}");
    let address = word.strip_prefix("http:").ok_or_else(bad)?;
    let address: SocketAddr = address.parse().map_err(|_| bad())?;
    if !address.ip().is_loopback() {
        return Err(format!(
            "--mcp {word}: this server has no authentication, so it binds loopback addresses \
             only; reach it from elsewhere through a tunnel, and name that with --mcp-host"
        ));
    }
    Ok(Transport::Http(address))
}

/// Parses everything after the program's name.
///
/// # Errors
/// A message for the user, on anything it does not understand.
pub fn parse(words: impl Iterator<Item = OsString>) -> Result<Args, String> {
    let mut args = Args {
        endpoint: Endpoint::platform_default(),
        transport: Transport::Stdio,
        hosts: Vec::new(),
        timeout: Duration::from_secs(30),
        help: false,
        version: false,
    };
    let mut words = words.map(|w| w.to_string_lossy().into_owned());
    while let Some(flag) = words.next() {
        match flag.as_str() {
            "-h" | "--help" => args.help = true,
            "-V" | "--version" => args.version = true,
            "--endpoint" | "--mcp" | "--mcp-host" | "--timeout" => {
                let word = words
                    .next()
                    .ok_or_else(|| format!("{flag} needs a value"))?;
                match flag.as_str() {
                    "--endpoint" => {
                        let parsed = word.parse();
                        args.endpoint = parsed.map_err(|e| format!("--endpoint: {e}"))?;
                    }
                    "--mcp" => args.transport = transport(&word)?,
                    "--mcp-host" => args.hosts.push(word),
                    _ => {
                        let seconds = word
                            .parse::<f64>()
                            .ok()
                            .filter(|s| *s > 0.0 && s.is_finite());
                        let seconds = seconds.ok_or("--timeout is a positive number of seconds")?;
                        let timeout = Duration::try_from_secs_f64(seconds);
                        args.timeout = timeout.map_err(|e| format!("--timeout: {e}"))?;
                    }
                }
            }
            other => return Err(format!("unexpected argument {other}")),
        }
    }
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn try_parse(words: &[&str]) -> Result<Args, String> {
        parse(words.iter().map(Into::into))
    }

    #[test]
    fn stdio_is_the_default_and_http_is_loopback_only() {
        assert_eq!(try_parse(&[]).unwrap().transport, Transport::Stdio);
        let args = try_parse(&["--mcp", "http:127.0.0.1:8090"]).unwrap();
        assert_eq!(
            args.transport,
            Transport::Http("127.0.0.1:8090".parse().unwrap())
        );
        assert!(try_parse(&["--mcp", "http:[::1]:8090"]).is_ok());
        let refused = try_parse(&["--mcp", "http:0.0.0.0:8090"]).unwrap_err();
        assert!(refused.contains("loopback"), "{refused}");
        assert!(try_parse(&["--mcp", "http:192.168.1.2:8090"]).is_err());
    }

    #[test]
    fn nonsense_is_refused_with_a_message() {
        for bad in [
            &["--mcp"][..],
            &["--mcp", "tcp:1"],
            &["--timeout", "0"],
            &["--timeout", "-1"],
            &["--endpoint", "tcp:10.0.0.1:1"],
            &["--frobnicate"],
        ] {
            assert!(try_parse(bad).is_err(), "{bad:?}");
        }
    }
}
