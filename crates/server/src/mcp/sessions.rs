//! Which replay sessions MCP opened. A read-only surface must not stop or move a
//! replay someone else is watching, so a tool names only sessions a tool created,
//! and `ListReplaySessions` shows only those. At most [`MAX_SESSIONS`] are open;
//! the daemon's own limit is shared with gRPC clients.

use std::collections::BTreeSet;
use std::sync::{Mutex, PoisonError};

use serde_json::Value;

/// Sessions MCP may hold open at once. Picked, not measured.
pub(super) const MAX_SESSIONS: usize = 8;

/// The sessions tools created. Correct from any task.
#[derive(Debug, Default)]
pub(super) struct Sessions(Mutex<BTreeSet<String>>);

impl Sessions {
    fn held(&self) -> std::sync::MutexGuard<'_, BTreeSet<String>> {
        self.0.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Whether `name` is one of them.
    pub(super) fn owns(&self, name: &str) -> bool {
        self.held().contains(name)
    }

    /// Every one held.
    pub(super) fn names(&self) -> Vec<String> {
        self.held().iter().cloned().collect()
    }

    /// How many are held.
    pub(super) fn count(&self) -> usize {
        self.held().len()
    }

    /// Records `name`.
    pub(super) fn add(&self, name: &str) {
        self.held().insert(name.to_owned());
    }

    /// Forgets `name`.
    pub(super) fn forget(&self, name: &str) {
        self.held().remove(name);
    }

    /// Keeps, in a `ListReplaySessions` answer, only the sessions held.
    pub(super) fn filter(&self, answer: &mut Value) {
        let held = self.held();
        if let Some(Value::Array(list)) = answer.get_mut("replaySessions") {
            list.retain(|s| {
                s.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| held.contains(n))
            });
        }
    }
}
