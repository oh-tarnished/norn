//! The clock controls: each a state transition, with the rules of protocol.md.

use std::sync::Arc;
use std::time::Duration;

use norn_core::Timestamp;
use norn_state::StateReader;

use super::{Goal, ReplayState, Session};
use crate::clock::Pace;
use crate::error::ReplayError;
use crate::event::{Batch, ClockKind, Event, Hold, MAX_BATCH_EVENTS, Payload};

/// A change to a session.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Control {
    /// Running becomes paused.
    Pause,
    /// Paused becomes running.
    Resume,
    /// Moves the clock, skipping what lies between.
    Seek {
        /// The next sample emitted is the first at or after this.
        to: Timestamp,
        /// Whether anyone is listening for the state there.
        report_state: bool,
    },
    /// Emits this many next sample events, or fewer if the interval ends.
    StepEvents(u64),
    /// Advances by this much virtual time, emitting everything before the new time.
    StepTime(Duration),
    /// Changes the pace.
    SetPace(Pace),
    /// Ends the session for good.
    Stop,
}

impl Control {
    fn name(self) -> &'static str {
        match self {
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::Seek { .. } => "seek",
            Self::StepEvents(_) | Self::StepTime(_) => "step",
            Self::SetPace(_) => "change the pace of",
            Self::Stop => "stop",
        }
    }
}

impl Session {
    /// Applies `control` at wall time `wall`, returning the events it causes, in
    /// order. A seek that reports state blocks on the disk.
    ///
    /// # Errors
    /// [`ReplayError::Conflict`] when the state forbids it, and nothing changes;
    /// [`ReplayError::OutsideInterval`]; [`ReplayError::Query`] when a seek cannot
    /// read the state, and the clock has not moved.
    pub fn control(
        &mut self,
        control: Control,
        wall: Timestamp,
    ) -> Result<Vec<Batch>, ReplayError> {
        use ReplayState as S;
        let conflict = ReplayError::Conflict {
            control: control.name(),
            state: self.state.word(),
        };
        let frozen = self.clock.pace() == Pace::Speed(0.0);
        let mut out = Vec::new();
        if self.goal.is_some() && control != Control::Stop {
            return Err(conflict);
        }
        match (control, self.state) {
            (Control::Stop, S::Stopped) | (Control::Pause, S::Paused) => {}
            (Control::Resume, S::Running) => {}
            (Control::Stop, _) => {
                self.goal = None;
                self.clock.hold(wall);
                self.state = S::Stopped;
                self.generation += 1;
            }
            (_, S::Stopped) => return Err(conflict),
            (Control::Pause, S::Running) => {
                self.clock.hold(wall);
                self.state = S::Paused;
                self.generation += 1;
                out.push(Batch::clock(ClockKind::Paused, self.clock.now(wall)));
            }
            (Control::Resume, S::Paused) => {
                self.clock.run(wall);
                self.state = S::Running;
                self.generation += 1;
                out.push(Batch::clock(ClockKind::Running, self.clock.now(wall)));
            }
            (Control::SetPace(pace), _) => {
                self.clock.set_pace(pace, wall);
                self.generation += 1;
                out.push(Batch::clock(ClockKind::SpeedChanged, self.clock.now(wall)));
            }
            (Control::Seek { to, report_state }, state) => {
                if to < self.start || to > self.end {
                    return Err(ReplayError::OutsideInterval);
                }
                out.push(Batch::clock(ClockKind::Seeked, to));
                if report_state {
                    out.extend(self.state_before(to)?);
                }
                self.cursor.seek(to);
                self.clock.set(to, wall);
                self.emitted = 0;
                self.generation += 1;
                if state == S::Ended {
                    self.state = S::Paused;
                }
            }
            (Control::StepEvents(n), S::Paused) => self.goal = Some(Goal::Events(n)),
            (Control::StepEvents(n), S::Running) if frozen => self.goal = Some(Goal::Events(n)),
            (Control::StepTime(by), state)
                if state == S::Paused || (state == S::Running && frozen) =>
            {
                let nanos = i64::try_from(by.as_nanos()).unwrap_or(i64::MAX);
                let now = self.clock.now(wall).as_nanos();
                let to = Timestamp::from_nanos(now.saturating_add(nanos)).min(self.end);
                self.goal = Some(Goal::Before(to));
            }
            _ => return Err(conflict),
        }
        Ok(out)
    }

    /// What each signal held just before `to`, as batches. `StateAt` is what a seek
    /// is built on: the sample at exactly `to` is emitted, so it is not state yet.
    fn state_before(&self, to: Timestamp) -> Result<Vec<Batch>, ReplayError> {
        let before = Timestamp::from_nanos(to.as_nanos().saturating_sub(1));
        let reader = StateReader::new(Arc::clone(self.cursor.engine()));
        let state = reader.state_at(self.cursor.targets().to_vec(), before, None)?;
        let state = Arc::new(state);
        let known = state.entries().iter().filter_map(|e| {
            let sample = e.sample.clone()?;
            let signal = Arc::clone(&e.signal);
            let payload = Payload::State { signal, sample };
            Some(Event {
                clock_time: to,
                payload,
            })
        });
        let events: Vec<Event> = known.collect();
        let hold: Hold = state;
        let batch = |list: &[Event]| Batch::new(list.to_vec(), Some(Arc::clone(&hold)));
        Ok(events.chunks(MAX_BATCH_EVENTS).map(batch).collect())
    }
}
