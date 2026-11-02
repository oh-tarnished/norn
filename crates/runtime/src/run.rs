//! `Runtime::run`: spawn, serve, drain, and stop with no orphans.

use std::collections::HashMap;
use std::sync::PoisonError;

use tokio::task::{JoinError, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::error::{RuntimeError, TaskError};
use crate::health::Health;
use crate::runtime::Runtime;

/// How a [`Runtime::run`] that did not fail ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct RunReport {
    /// Tasks still running when the shutdown timeout expired, and therefore
    /// aborted. Zero means every task stopped on its own.
    pub aborted: usize,
}

type Joined = Result<(&'static str, Result<(), TaskError>), JoinError>;

impl Runtime {
    /// Runs every registered task until `cancel` is cancelled, [`close`]
    /// is called, or a task fails; then drains and stops them all.
    ///
    /// # Shutdown
    ///
    /// Health becomes [`Health::Draining`] and every task's token is
    /// cancelled. Tasks get the shutdown timeout (on the runtime's clock) to
    /// return; any still running are then aborted at their next `.await`
    /// and counted in [`RunReport::aborted`]. Health becomes
    /// [`Health::Stopped`], the budgets are closed, and only then does
    /// `run` return: no task it spawned outlives it. Dropping the `run`
    /// future also aborts every task, since they live in its `JoinSet`.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::AlreadyStarted`] on a second call,
    /// [`RuntimeError::Closed`] after `close`, and
    /// [`RuntimeError::TaskFailed`] or [`RuntimeError::TaskPanicked`] for
    /// the first task to fail, after the others have stopped.
    ///
    /// [`close`]: Runtime::close
    pub async fn run(&self, cancel: CancellationToken) -> Result<RunReport, RuntimeError> {
        let tasks = {
            let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
            if self.closed.is_cancelled() {
                return Err(RuntimeError::Closed);
            }
            guard.take().ok_or(RuntimeError::AlreadyStarted)?
        };
        let stop = CancellationToken::new();
        let mut set = JoinSet::new();
        let mut names = HashMap::with_capacity(tasks.len());
        for (name, task) in tasks {
            let fut = task(stop.child_token());
            let handle = set.spawn(async move { (name, fut.await) });
            names.insert(handle.id(), name);
        }
        self.health.send_replace(Health::Serving);
        tracing::info!(tasks = set.len(), "runtime serving");

        let mut failure = None;
        while failure.is_none() {
            tokio::select! {
                () = cancel.cancelled() => break,
                () = self.closed.cancelled() => break,
                Some(joined) = set.join_next(), if !set.is_empty() => {
                    failure = outcome(joined, &names).err();
                }
            }
        }

        self.health.send_replace(Health::Draining);
        tracing::info!(tasks = set.len(), "runtime draining");
        stop.cancel();
        let mut deadline = self.clock.sleep(self.shutdown_timeout);
        let mut aborted = 0;
        loop {
            tokio::select! {
                joined = set.join_next() => match joined {
                    None => break,
                    Some(joined) => {
                        let result = outcome(joined, &names);
                        if failure.is_none() {
                            failure = result.err();
                        }
                    }
                },
                () = &mut deadline => {
                    aborted = set.len();
                    tracing::warn!(aborted, "shutdown timeout: aborting tasks");
                    set.abort_all();
                    while set.join_next().await.is_some() {}
                    break;
                }
            }
        }
        self.budgets.close();
        self.health.send_replace(Health::Stopped);
        tracing::info!(aborted, "runtime stopped");
        match failure {
            Some(error) => Err(error),
            None => Ok(RunReport { aborted }),
        }
    }
}

/// Turns one joined task into `Ok` or the fatal error it represents.
fn outcome(
    joined: Joined,
    names: &HashMap<tokio::task::Id, &'static str>,
) -> Result<(), RuntimeError> {
    match joined {
        Ok((_, Ok(()))) => Ok(()),
        Ok((task, Err(source))) => {
            tracing::error!(task, error = %source, "task failed");
            Err(RuntimeError::TaskFailed { task, source })
        }
        Err(e) if e.is_cancelled() => Ok(()),
        Err(e) => {
            let task = names.get(&e.id()).copied().unwrap_or("?");
            tracing::error!(task, "task panicked");
            Err(RuntimeError::TaskPanicked { task })
        }
    }
}
