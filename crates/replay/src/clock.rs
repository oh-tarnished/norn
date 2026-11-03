//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The virtual clock: independent of wall time, told the wall time when it matters.

use std::time::Duration;

use norn_core::Timestamp;

/// How fast a clock runs while it runs.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Pace {
    /// A multiple of real time; 0 freezes the clock, so only a step moves it.
    Speed(f64),
    /// The clock jumps to each next event, as fast as subscribers read.
    Maximum,
}

impl Pace {
    /// A speed, if it is finite and not negative.
    #[must_use]
    pub fn speed(multiple: f64) -> Option<Self> {
        (multiple.is_finite() && multiple >= 0.0).then_some(Self::Speed(multiple))
    }
}

impl Default for Pace {
    /// Real time, what an unset pace means.
    fn default() -> Self {
        Self::Speed(1.0)
    }
}

/// A clock reading virtual time, advanced by nothing but what it is told.
///
/// While running at a speed it is a line through an anchor: `virtual = anchor +
/// (wall - wall anchor) × speed`. Every control moves the anchor. At
/// [`Pace::Maximum`] or while held, it stays where it was last [`set`](Self::set).
///
/// `Copy`, so a read-only copy can be published for callers to read without
/// asking the session's driver. The arithmetic is in `f64` of nanoseconds, good to
/// about 100 ns over a year of running: enough for pacing, and nothing about an
/// event's identity or order depends on it.
///
/// Concurrency: plain data. Failure: saturates rather than overflows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VirtualClock {
    wall: Timestamp,
    at: Timestamp,
    pace: Pace,
    running: bool,
}

impl VirtualClock {
    /// A held clock reading `at`.
    #[must_use]
    pub fn new(at: Timestamp, pace: Pace) -> Self {
        Self {
            wall: Timestamp::from_nanos(0),
            at,
            pace,
            running: false,
        }
    }

    /// The clock's pace.
    #[must_use]
    pub fn pace(&self) -> Pace {
        self.pace
    }

    /// The reading at wall time `wall`.
    #[must_use]
    pub fn now(&self, wall: Timestamp) -> Timestamp {
        match (self.running, self.pace) {
            (true, Pace::Speed(speed)) => {
                let spent = wall.as_nanos().saturating_sub(self.wall.as_nanos()).max(0);
                let moved = (spent as f64 * speed) as i64;
                Timestamp::from_nanos(self.at.as_nanos().saturating_add(moved))
            }
            _ => self.at,
        }
    }

    /// Starts it, from where it reads.
    pub fn run(&mut self, wall: Timestamp) {
        self.at = self.now(wall);
        self.wall = wall;
        self.running = true;
    }

    /// Stops it where it reads.
    pub fn hold(&mut self, wall: Timestamp) {
        self.at = self.now(wall);
        self.wall = wall;
        self.running = false;
    }

    /// Moves it to `at`, running or held as it was.
    pub fn set(&mut self, at: Timestamp, wall: Timestamp) {
        self.at = at;
        self.wall = wall;
    }

    /// Changes its pace, from where it reads.
    pub fn set_pace(&mut self, pace: Pace, wall: Timestamp) {
        self.at = self.now(wall);
        self.wall = wall;
        self.pace = pace;
    }

    /// The wall time until it reads `target`. Zero if it already does; `None` if it
    /// never will by itself: held, frozen, or at [`Pace::Maximum`].
    #[must_use]
    pub fn until(&self, target: Timestamp, wall: Timestamp) -> Option<Duration> {
        let now = self.now(wall);
        if now >= target {
            return Some(Duration::ZERO);
        }
        match (self.running, self.pace) {
            (true, Pace::Speed(speed)) if speed > 0.0 => {
                let left = target.as_nanos().saturating_sub(now.as_nanos()) as f64 / speed;
                // At least 1 ns, so a rounding error cannot spin.
                Some(Duration::from_nanos((left.ceil() as u64).max(1)))
            }
            _ => None,
        }
    }
}
