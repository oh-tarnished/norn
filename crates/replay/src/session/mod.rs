//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One replay: its clock, its cursor and its state.

mod control;
mod next;

pub use control::Control;
pub use next::Next;

use std::sync::Arc;

use norn_core::Timestamp;
use norn_query::{Engine, Target};

use crate::clock::{Pace, VirtualClock};
use crate::cursor::Cursor;

/// Whether a session's clock is advancing.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ReplayState {
    /// Stopped where it is. Every session starts here.
    Paused,
    /// Advancing at its pace.
    Running,
    /// At the end of the interval; a seek makes it paused again.
    Ended,
    /// Stopped for good.
    Stopped,
}

impl ReplayState {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Self::Paused => "paused",
            Self::Running => "running",
            Self::Ended => "ended",
            Self::Stopped => "stopped",
        }
    }
}

/// What a session replays. Fixed at creation.
///
/// Concurrency: plain owned data.
#[derive(Debug, Clone)]
pub struct SessionSpec {
    /// The first instant replayed.
    pub start: Timestamp,
    /// The instant past the last one replayed.
    pub end: Timestamp,
    /// The signals, resolved once: one created later does not join.
    pub targets: Vec<Target>,
    /// How fast to run.
    pub pace: Pace,
}

/// What a session looks like from outside, published after every change.
///
/// `Copy`, so a caller can read a session without asking its driver.
/// Concurrency: plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SessionView {
    /// Whether the clock is advancing.
    pub state: ReplayState,
    /// The clock. [`VirtualClock::now`] gives the current reading.
    pub clock: VirtualClock,
    /// Sample events since the last seek or creation.
    pub emitted: u64,
    /// Changes whenever the controls or the state do; what an etag is made of.
    pub generation: u64,
}

/// How far a step still has to go.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Goal {
    Events(u64),
    Before(Timestamp),
}

/// The state machine of one replay.
///
/// Told the wall time, it answers with what to do ([`Next`]); it reads no clock and
/// spawns nothing, so a driver with a fake clock runs it exactly as the daemon does.
///
/// Lifecycle: [`Session::new`] reads nothing. Dropping it at any point is the whole
/// of ending it. Concurrency: `Send`, one driver. [`fill`](Session::fill) and
/// [`control`](Session::control) can block on the disk. Memory: see the crate root.
/// Failure: see [`Next`].
#[derive(Debug)]
pub struct Session {
    start: Timestamp,
    end: Timestamp,
    clock: VirtualClock,
    state: ReplayState,
    cursor: Cursor,
    emitted: u64,
    generation: u64,
    goal: Option<Goal>,
}

impl Session {
    /// A session, paused at the start of `spec`, reading through `engine`.
    #[must_use]
    pub fn new(spec: SessionSpec, engine: Arc<Engine>) -> Self {
        let SessionSpec {
            start,
            end,
            targets,
            pace,
        } = spec;
        Self {
            start,
            end,
            clock: VirtualClock::new(start, pace),
            state: ReplayState::Paused,
            cursor: Cursor::new(engine, targets, start, end),
            emitted: 0,
            generation: 0,
            goal: None,
        }
    }

    /// What it looks like now.
    #[must_use]
    pub fn view(&self) -> SessionView {
        SessionView {
            state: self.state,
            clock: self.clock,
            emitted: self.emitted,
            generation: self.generation,
        }
    }

    /// Whether a step is under way.
    #[must_use]
    pub fn stepping(&self) -> bool {
        self.goal.is_some()
    }

    /// The session's interval.
    #[must_use]
    pub fn interval(&self) -> (Timestamp, Timestamp) {
        (self.start, self.end)
    }

    /// Freezes the clock while a subscriber catches up: it slows to the slowest.
    pub fn hold(&mut self, wall: Timestamp) {
        if self.state == ReplayState::Running {
            self.clock.hold(wall);
        }
    }

    /// Lets the clock go on after [`hold`](Self::hold).
    pub fn release(&mut self, wall: Timestamp) {
        if self.state == ReplayState::Running {
            self.clock.run(wall);
        }
    }

    /// Reads the next page, when [`Next::Read`] asked for it. Blocks on the disk.
    ///
    /// # Errors
    /// [`crate::ReplayError::Query`]. The session pauses where it stands: nothing
    /// of the failed slice was emitted, and a retry or a seek carries on.
    pub fn fill(&mut self, wall: Timestamp) -> Result<(), crate::ReplayError> {
        self.cursor.fill().inspect_err(|_| {
            self.goal = None;
            if self.state == ReplayState::Running {
                self.clock.hold(wall);
                self.state = ReplayState::Paused;
                self.generation += 1;
            }
        })
    }
}
