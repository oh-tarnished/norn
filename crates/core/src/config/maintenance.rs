//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! When the daemon's background upkeep runs.

use std::time::Duration;

/// The schedule of compaction and retention (SPEC §42, §43).
///
/// `#[non_exhaustive]`: start from [`Default`] and set fields.
///
/// `Default` (picked, not measured; stage 15 revisits them): a compaction pass
/// every 5 minutes of at most 8 merges a stream, at any hour; a retention pass
/// every hour. A stream's policy defaults to keeping everything, so the default
/// retention pass deletes nothing.
///
/// Invariant (checked by [`crate::ConfigBuilder::build`]): both intervals and
/// `max_runs_per_pass` are non-zero, and a window's minutes are below 1,440.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct MaintenanceConfig {
    /// Whether passes run by themselves. `false` leaves only what a client asks
    /// for through `CompactStream` and `ApplyStreamRetention`.
    pub background: bool,
    /// How long between compaction passes.
    pub compaction_interval: Duration,
    /// How long between retention passes.
    pub retention_interval: Duration,
    /// The most merges one pass does per stream. Bounds how long one stream can
    /// hold the worker.
    pub max_runs_per_pass: usize,
    /// The part of the UTC day background compaction may start in, as minutes
    /// from midnight, start then end; it wraps past midnight when start is later.
    /// `None` is all day. A client's `CompactStream` ignores it.
    pub compaction_window: Option<(u16, u16)>,
}

impl MaintenanceConfig {
    /// The name of the first setting that is out of range, for validation.
    pub(crate) fn first_invalid(&self) -> Option<&'static str> {
        const DAY: u16 = 24 * 60;
        if self.compaction_interval.is_zero() {
            Some("compaction_interval")
        } else if self.retention_interval.is_zero() {
            Some("retention_interval")
        } else if self.max_runs_per_pass == 0 {
            Some("max_runs_per_pass")
        } else if self
            .compaction_window
            .is_some_and(|(a, b)| a >= DAY || b >= DAY)
        {
            Some("compaction_window")
        } else {
            None
        }
    }

    /// Whether minute `of_day` (0 to 1,439, UTC) lies in the window. Pure.
    #[must_use]
    pub fn in_window(&self, of_day: u16) -> bool {
        match self.compaction_window {
            None => true,
            Some((start, end)) if start <= end => start <= of_day && of_day < end,
            Some((start, end)) => of_day >= start || of_day < end,
        }
    }
}

impl Default for MaintenanceConfig {
    fn default() -> Self {
        Self {
            background: true,
            compaction_interval: Duration::from_secs(300),
            retention_interval: Duration::from_secs(3600),
            max_runs_per_pass: 8,
            compaction_window: None,
        }
    }
}
