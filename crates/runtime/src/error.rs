//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The runtime's error model.

use std::fmt;

/// What a task returns when it fails. Boxed so that each layer's own typed
/// error passes through unchanged; downcast to recover it.
pub type TaskError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Everything constructing or running a [`Runtime`](crate::Runtime) can fail
/// with. Match on the variant, never on the message.
#[derive(Debug)]
#[non_exhaustive]
pub enum RuntimeError {
    /// A memory budget is larger than any allocation this platform can make
    /// (`isize::MAX` bytes), so honouring it would bound nothing.
    BudgetTooLarge {
        /// The budget's name, such as `total_bytes`.
        budget: &'static str,
        /// The configured size.
        bytes: u64,
    },
    /// `run` was called a second time, or `add_task` after `run` started.
    /// A runtime runs once; build a new one to run again.
    AlreadyStarted,
    /// The runtime was closed before `run`, so there is nothing to run.
    Closed,
    /// More tasks than [`MAX_TASKS`](crate::MAX_TASKS) were added.
    TooManyTasks {
        /// The limit.
        limit: usize,
    },
    /// A task returned an error, which is fatal: every other task was told
    /// to stop and has stopped by the time this is returned.
    TaskFailed {
        /// The name it was added under.
        task: &'static str,
        /// What it returned.
        source: TaskError,
    },
    /// A task panicked. Treated like [`TaskFailed`](Self::TaskFailed): the
    /// rest were stopped before this is returned.
    TaskPanicked {
        /// The name it was added under.
        task: &'static str,
    },
}

impl fmt::Display for RuntimeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BudgetTooLarge { budget, bytes } => {
                write!(
                    f,
                    "memory budget {budget} of {bytes} bytes exceeds isize::MAX"
                )
            }
            Self::AlreadyStarted => f.write_str("the runtime has already started"),
            Self::Closed => f.write_str("the runtime was closed before it ran"),
            Self::TooManyTasks { limit } => write!(f, "more than {limit} tasks"),
            Self::TaskFailed { task, source } => write!(f, "task {task} failed: {source}"),
            Self::TaskPanicked { task } => write!(f, "task {task} panicked"),
        }
    }
}

impl std::error::Error for RuntimeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::TaskFailed { source, .. } => Some(source.as_ref()),
            _ => None,
        }
    }
}
