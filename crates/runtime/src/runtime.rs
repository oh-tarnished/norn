//! Constructing a runtime and registering its tasks; running is `run.rs`.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use norn_core::{Config, Timestamp};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::budget::Budgets;
use crate::clock::{Clock, SystemClock};
use crate::error::{RuntimeError, TaskError};
use crate::health::{Health, HealthWatch};

/// The most tasks one runtime runs. Tasks are added by the daemon's own
/// code, never by clients, so this is a guard against a bug, not a tuning
/// knob.
pub const MAX_TASKS: usize = 64;

/// How long `run` waits for tasks to stop after cancellation before
/// aborting them, unless set with [`Runtime::shutdown_timeout`]. Longer
/// than norn-ipc's 10 s drain plus its 1 s abort grace, so a server task
/// finishes its own bounded drain before the runtime gives up on it.
pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(15);

pub(crate) type TaskFuture = Pin<Box<dyn Future<Output = Result<(), TaskError>> + Send>>;
pub(crate) type TaskFn = Box<dyn FnOnce(CancellationToken) -> TaskFuture + Send>;

/// The daemon's lifecycle: health, memory budgets, and the tasks it owns.
///
/// It knows nothing of networks or protocols; the server crate adds a task
/// that serves gRPC. Lifecycle:
///
/// - [`new`](Runtime::new) validates and constructs. **It spawns nothing**:
///   a runtime can be built, inspected and dropped with no effect.
/// - [`add_task`](Runtime::add_task) registers work to start on `run`.
/// - [`run`](Runtime::run) spawns every task, reports
///   [`Health::Serving`], and blocks until cancelled, closed, or a task
///   fails. It then reports [`Health::Draining`], cancels the tasks'
///   token, waits up to the shutdown timeout, aborts any task still
///   running, and returns only once every task has ended: no orphans.
/// - [`close`](Runtime::close) is idempotent and safe at any point.
///
/// Concurrency: `Send + Sync`. Share it by reference or `Arc`; `close` and
/// the accessors may be called from any task while `run` is in progress.
pub struct Runtime {
    pub(crate) config: Config,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) budgets: Budgets,
    pub(crate) health: watch::Sender<Health>,
    pub(crate) tasks: Mutex<Option<Vec<(&'static str, TaskFn)>>>,
    pub(crate) closed: CancellationToken,
    pub(crate) shutdown_timeout: Duration,
    start_time: Timestamp,
}

impl Runtime {
    /// A runtime for `config` on the system clock.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::BudgetTooLarge`] if a memory budget exceeds
    /// `isize::MAX` bytes on this platform.
    pub fn new(config: Config) -> Result<Self, RuntimeError> {
        Self::with_clock(config, Arc::new(SystemClock))
    }

    /// As [`new`](Runtime::new), with an injected clock (a `FakeClock` in
    /// tests, a replay clock later). Every timeout and timestamp the runtime
    /// and its budgets produce comes from `clock`.
    ///
    /// # Errors
    ///
    /// As [`new`](Runtime::new).
    pub fn with_clock(config: Config, clock: Arc<dyn Clock>) -> Result<Self, RuntimeError> {
        let memory = config.memory();
        let limit = u64::try_from(isize::MAX).unwrap_or(u64::MAX);
        let all = std::iter::once(("total_bytes", memory.total_bytes)).chain(memory.subsystems());
        for (budget, bytes) in all {
            if bytes > limit {
                return Err(RuntimeError::BudgetTooLarge { budget, bytes });
            }
        }
        let (health, _) = watch::channel(Health::Starting);
        Ok(Self {
            budgets: Budgets::new(memory, &clock),
            start_time: clock.now(),
            config,
            clock,
            health,
            tasks: Mutex::new(Some(Vec::new())),
            closed: CancellationToken::new(),
            shutdown_timeout: DEFAULT_SHUTDOWN_TIMEOUT,
        })
    }

    /// Sets how long `run` waits for tasks after cancellation before it
    /// aborts them. Default [`DEFAULT_SHUTDOWN_TIMEOUT`].
    #[must_use]
    pub fn shutdown_timeout(mut self, timeout: Duration) -> Self {
        self.shutdown_timeout = timeout;
        self
    }

    /// Registers `task`, to be spawned when [`run`](Runtime::run) starts.
    ///
    /// The task receives a `CancellationToken` that is cancelled when the
    /// runtime begins draining; it must then finish its in-flight work and
    /// return. Returning `Ok` early is allowed (the runtime keeps running);
    /// returning `Err` is fatal and stops the whole runtime.
    ///
    /// # Errors
    ///
    /// [`RuntimeError::AlreadyStarted`] once `run` has begun or `close` was
    /// called; [`RuntimeError::TooManyTasks`] past [`MAX_TASKS`].
    pub fn add_task<F, Fut>(&self, name: &'static str, task: F) -> Result<(), RuntimeError>
    where
        F: FnOnce(CancellationToken) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), TaskError>> + Send + 'static,
    {
        let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        let tasks = guard.as_mut().ok_or(RuntimeError::AlreadyStarted)?;
        if tasks.len() >= MAX_TASKS {
            return Err(RuntimeError::TooManyTasks { limit: MAX_TASKS });
        }
        tasks.push((name, Box::new(move |token| Box::pin(task(token)))));
        Ok(())
    }

    /// Stops the runtime. Idempotent, and safe before, during or after
    /// `run`, including after a failed one.
    ///
    /// Before `run`: discards the registered tasks unstarted and reports
    /// [`Health::Stopped`]; a later `run` returns [`RuntimeError::Closed`].
    /// During `run`: requests shutdown exactly as cancelling `run`'s token
    /// does, and returns at once; `run` returning is the confirmation that
    /// every task has stopped.
    pub fn close(&self) {
        self.closed.cancel();
        let mut guard = self.tasks.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.take().is_some() {
            self.health.send_replace(Health::Stopped);
        }
    }

    /// A watch on the runtime's [`Health`].
    #[must_use]
    pub fn health(&self) -> HealthWatch {
        HealthWatch::new(self.health.subscribe())
    }

    /// The per-subsystem memory budgets. Clone a [`Budget`](crate::Budget)
    /// to hand it to a subsystem; clones share the account.
    #[must_use]
    pub fn budgets(&self) -> &Budgets {
        &self.budgets
    }

    /// The configuration it was built with.
    #[must_use]
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// The clock it was built with.
    #[must_use]
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.clock
    }

    /// When it was constructed, on its clock.
    #[must_use]
    pub fn start_time(&self) -> Timestamp {
        self.start_time
    }
}
