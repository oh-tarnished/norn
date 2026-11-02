//! The sessions this server opened.

use std::collections::BTreeSet;
use std::sync::{Arc, Mutex, MutexGuard};

use super::{delete, get};
use crate::daemon::Call;
use crate::error::ToolError;

/// The most sessions this server holds open: under the daemon's default 16, which
/// other clients share. Picked, not measured.
const MAX_SESSIONS: usize = 8;

/// The sessions this server opened. Shared by every MCP client of this process.
///
/// Concurrency: `Clone`, shared; the lock is never held across an await. Memory: at
/// most [`MAX_SESSIONS`] names.
#[derive(Debug, Clone, Default)]
pub(crate) struct Sessions(Arc<Mutex<BTreeSet<String>>>);

impl Sessions {
    fn lock(&self) -> MutexGuard<'_, BTreeSet<String>> {
        self.0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    pub(crate) fn add(&self, name: &str) {
        self.lock().insert(name.to_owned());
    }

    pub(crate) fn remove(&self, name: &str) {
        self.lock().remove(name);
    }

    pub(crate) fn names(&self) -> Vec<String> {
        self.lock().iter().cloned().collect()
    }

    /// Fails unless this server opened `name`.
    pub(crate) fn owned(&self, name: &str) -> Result<(), ToolError> {
        if self.lock().contains(name) {
            return Ok(());
        }
        let mut e = ToolError::new("MCP_UNKNOWN_SESSION", format!("{name} is not open here"));
        e.hint = Some("Use a name from `start`, or action `list`.".to_owned());
        Err(e)
    }

    /// Makes room for one more, forgetting sessions the daemon has expired; fails if
    /// all are still live, rather than stopping a session that may be in use.
    pub(crate) async fn room(&self, call: &Call) -> Result<(), ToolError> {
        if self.lock().len() < MAX_SESSIONS {
            return Ok(());
        }
        for name in self.names() {
            match get(call, &name).await {
                Err(e) if e.reason == "ERROR_REASON_RESOURCE_NOT_FOUND" => self.remove(&name),
                _ => {}
            }
        }
        if self.lock().len() < MAX_SESSIONS {
            return Ok(());
        }
        let hint = "Stop a session you have finished with; `list` names them.";
        Err(ToolError::limit(
            "maximum open replay sessions",
            MAX_SESSIONS,
            hint,
        ))
    }

    /// Deletes every session, at shutdown. Best effort: a daemon that is gone has no
    /// sessions, and one that is not answering expires them itself.
    pub(crate) async fn close(&self, call: &Call) {
        for name in self.names() {
            let _ = delete(call, &name).await;
            self.remove(&name);
        }
    }
}
