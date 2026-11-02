//! Running the server on the chosen transport until told to stop.

use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;

use crate::args::{Args, Transport};
use crate::daemon::Daemon;
use crate::http;
use crate::server::NornMcp;

/// Resolves on the first signal to stop.
#[cfg(unix)]
async fn terminated() {
    use tokio::signal::unix::{SignalKind, signal};
    let Ok(mut term) = signal(SignalKind::terminate()) else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = tokio::signal::ctrl_c() => {}
    }
}

/// Resolves on the first signal to stop.
#[cfg(not(unix))]
async fn terminated() {
    let _ = tokio::signal::ctrl_c().await;
}

async fn on_stdio(server: NornMcp, cancel: CancellationToken) -> Result<(), String> {
    let running = server.serve(stdio()).await;
    let running = running.map_err(|e| format!("cannot start MCP on stdio: {e}"))?;
    let stop = running.cancellation_token();
    tokio::select! {
        ended = running.waiting() => {
            ended.map_err(|e| format!("MCP on stdio failed: {e}")).map(drop)
        }
        () = cancel.cancelled() => {
            stop.cancel();
            Ok(())
        }
    }
}

/// Serves until the stdio client leaves or a signal arrives, then stops every replay
/// session this server opened.
///
/// # Errors
/// A message for the user: the HTTP address is taken, or MCP could not start.
pub async fn run(args: Args) -> Result<(), String> {
    let server = NornMcp::new(Daemon::new(&args.endpoint, args.timeout));
    let cancel = CancellationToken::new();
    let result = {
        let watcher = cancel.clone();
        let signals = tokio::spawn(async move {
            terminated().await;
            watcher.cancel();
        });
        let result = match args.transport {
            Transport::Stdio => on_stdio(server.clone(), cancel.clone()).await,
            Transport::Http(address) => {
                let bound = TcpListener::bind(address).await;
                let bound = bound.map_err(|e| format!("cannot bind {address}: {e}"));
                match bound {
                    Ok(listener) => {
                        let local = listener.local_addr().map_err(|e| e.to_string())?;
                        eprintln!("norn-mcp: serving http://{local}/mcp");
                        http::serve(listener, server.clone(), &args.hosts, cancel.clone()).await;
                        Ok(())
                    }
                    Err(e) => Err(e),
                }
            }
        };
        signals.abort();
        let _ = signals.await;
        result
    };
    server.close().await;
    result
}
