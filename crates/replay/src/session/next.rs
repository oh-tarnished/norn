//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a session wants done next.

use std::time::Duration;

use norn_core::Timestamp;

use super::{Goal, ReplayState, Session};
use crate::clock::Pace;
use crate::event::{Batch, ClockKind, Event, MAX_BATCH_EVENTS, Payload};

/// What a driver does after [`Session::poll`].
///
/// Concurrency: plain owned data.
#[derive(Debug)]
#[non_exhaustive]
pub enum Next {
    /// Deliver these to every subscriber, then poll again.
    Emit(Batch),
    /// Nothing is due for this long of wall time. A control may change that.
    Wait(Duration),
    /// Call [`Session::fill`] where blocking is allowed, then poll again.
    Read,
    /// Nothing happens until a control says so.
    Idle,
}

/// Up to where events are due.
#[derive(Clone, Copy)]
enum Due {
    Any,
    Before(Timestamp),
    Until(Timestamp),
}

impl Due {
    fn covers(self, t: Timestamp) -> bool {
        match self {
            Self::Any => true,
            Self::Before(limit) => t < limit,
            Self::Until(limit) => t <= limit,
        }
    }
}

impl Session {
    fn due(&self, wall: Timestamp) -> Option<Due> {
        match (self.goal, self.state) {
            (Some(Goal::Events(_)), _) => Some(Due::Any),
            (Some(Goal::Before(t)), _) => Some(Due::Before(t)),
            (None, ReplayState::Running) => match self.clock.pace() {
                Pace::Maximum => Some(Due::Any),
                Pace::Speed(0.0) => None,
                Pace::Speed(_) => Some(Due::Until(self.clock.now(wall))),
            },
            (None, _) => None,
        }
    }

    /// The events due at wall time `wall`, or what else to do.
    pub fn poll(&mut self, wall: Timestamp) -> Next {
        if self.goal == Some(Goal::Events(0)) {
            self.goal = None;
        }
        let Some(due) = self.due(wall) else {
            return Next::Idle;
        };
        if self.cursor.peek().is_none() && !self.cursor.exhausted() {
            return Next::Read;
        }
        let batch = self.take(due);
        if !batch.is_empty() {
            let last = batch.iter().rev().map(|e| e.clock_time).next();
            let realtime = self.goal.is_none() && matches!(self.clock.pace(), Pace::Speed(_));
            if let (Some(last), false) = (last, realtime) {
                self.clock.set(last, wall);
            }
            return Next::Emit(Batch::new(batch, self.cursor.hold()));
        }
        self.settle(wall)
    }

    fn take(&mut self, due: Due) -> Vec<Event> {
        let mut most = MAX_BATCH_EVENTS as u64;
        if let Some(Goal::Events(left)) = self.goal {
            most = most.min(left);
        }
        let mut events = Vec::new();
        while (events.len() as u64) < most {
            if !self.cursor.peek().is_some_and(|t| due.covers(t)) {
                break;
            }
            let Some((signal, sample)) = self.cursor.pop() else {
                break;
            };
            let clock_time = sample.timestamp;
            let payload = Payload::Sample { signal, sample };
            events.push(Event {
                clock_time,
                payload,
            });
        }
        self.emitted += events.len() as u64;
        if let Some(Goal::Events(left)) = &mut self.goal {
            *left -= events.len() as u64;
        }
        events
    }

    /// Nothing is due: wait for what is next, finish a step, or finish altogether.
    fn settle(&mut self, wall: Timestamp) -> Next {
        let next = self.cursor.peek();
        match self.goal {
            Some(Goal::Before(t)) if next.is_some() || t < self.end => {
                self.goal = None;
                self.clock.set(t, wall);
                return Next::Idle;
            }
            Some(_) => return self.finish(wall),
            None => {}
        }
        match (next, self.clock.pace()) {
            (None, Pace::Maximum) => self.finish(wall),
            (next, _) => match self.clock.until(next.unwrap_or(self.end), wall) {
                Some(Duration::ZERO) if next.is_none() => self.finish(wall),
                Some(wait) => Next::Wait(wait.max(Duration::from_nanos(1))),
                None => Next::Idle,
            },
        }
    }

    fn finish(&mut self, wall: Timestamp) -> Next {
        self.goal = None;
        self.clock.set(self.end, wall);
        self.clock.hold(wall);
        self.state = ReplayState::Ended;
        self.generation += 1;
        Next::Emit(Batch::clock(ClockKind::Ended, self.end))
    }
}
