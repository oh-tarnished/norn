//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The two small things a sink keeps: how long to wait, and what happened.

use std::time::Duration;

/// The first wait after a failure with no delay of its own.
const FIRST_BACKOFF: Duration = Duration::from_millis(250);
/// The longest. A daemon restarting is back in seconds, and a sink that waited
/// longer than this would be holding readings for nothing.
const MAX_BACKOFF: Duration = Duration::from_secs(30);

/// A wait that doubles until something works. Plain data.
#[derive(Clone, Debug)]
pub(crate) struct Backoff(Duration);

impl Default for Backoff {
    fn default() -> Self {
        Self(FIRST_BACKOFF)
    }
}

impl Backoff {
    /// The next wait, doubling the one after.
    pub(crate) fn next_wait(&mut self) -> Duration {
        let wait = self.0;
        self.0 = wait.saturating_mul(2).min(MAX_BACKOFF);
        wait
    }

    /// Something worked: the next wait is the first again.
    pub(crate) fn reset(&mut self) {
        self.0 = FIRST_BACKOFF;
    }
}

/// What a sink has counted since `new`. Every reading pushed ends in exactly
/// one of `acknowledged`, `overflowed`, `gated`, `invalid` or a report's
/// `never_sent`, or is still held.
///
/// Plain data; [`Default`] is nothing yet.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Counts {
    /// Readings acknowledged at the level asked for.
    pub acknowledged: u64,
    /// Of those, the ones Norn already held: resends after a lost call.
    pub duplicates: u64,
    /// Batches admitted with `THROTTLE`.
    pub throttled: u64,
    /// Batches refused and held for another try.
    pub rejected: u64,
    /// Readings given up when the buffer reached its byte bound, oldest first.
    pub overflowed: u64,
    /// Readings given up because their signal held its share of the buffer:
    /// one chatty signal, kept from starving the rest.
    pub gated: u64,
    /// Readings Norn called invalid; every other in their batch went on.
    pub invalid: u64,
}

impl Counts {
    /// Readings given up for want of room: `overflowed` and `gated`.
    #[must_use]
    pub fn dropped(&self) -> u64 {
        self.overflowed + self.gated
    }
}

#[cfg(test)]
mod tests {
    use super::Backoff;

    #[test]
    fn the_backoff_doubles_stops_at_its_ceiling_and_starts_over() {
        let mut b = Backoff::default();
        let ms: Vec<_> = (0..9).map(|_| b.next_wait().as_millis()).collect();
        assert_eq!(ms, [250, 500, 1000, 2000, 4000, 8000, 16000, 30000, 30000]);
        b.reset();
        assert_eq!(b.next_wait().as_millis(), 250);
    }
}
