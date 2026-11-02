//! `norn-mcp`: see the `norn_mcp` library.
//!
//! Exit status: 0 when a client leaves or a signal stops it, 1 when serving failed, 2
//! for a malformed command line.

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use tracing_subscriber::filter::LevelFilter;

/// stderr, where a failed write is dropped: a client that closed this process's
/// stderr must not take down the request whose warning could not be written.
struct Quiet;

impl Write for Quiet {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let _ = std::io::stderr().write_all(buf);
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        let _ = std::io::stderr().flush();
        Ok(())
    }
}

fn main() -> ExitCode {
    let args = match norn_mcp::parse(std::env::args_os().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("norn-mcp: {message}\n\n{}", norn_mcp::USAGE);
            return ExitCode::from(2);
        }
    };
    if args.help {
        println!("{}", norn_mcp::USAGE);
        return ExitCode::SUCCESS;
    }
    if args.version {
        println!("norn-mcp {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    // stderr always: stdout is the stdio transport, and a stray line would break it.
    let _ = tracing_subscriber::fmt()
        .with_writer(|| Quiet)
        .with_max_level(LevelFilter::WARN)
        .try_init();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => {
            eprintln!("norn-mcp: cannot start the async runtime: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = runtime.block_on(norn_mcp::run(args));
    // A read of stdin blocks a thread nothing can interrupt, so do not wait for it.
    runtime.shutdown_timeout(Duration::from_secs(1));
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("norn-mcp: {message}");
            ExitCode::FAILURE
        }
    }
}
