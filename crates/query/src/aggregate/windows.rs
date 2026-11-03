//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Fixed windows: which one a timestamp falls in, and where each lies.

use norn_core::Timestamp;

use crate::spec::TimeRange;

/// The windows of one query.
///
/// Aligned to the interval's start, or to the Unix epoch when it has none, so a
/// window's identity never depends on the data. The last is clipped to the
/// interval's end. `width: None` is one window over the whole interval.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Windows {
    range: TimeRange,
    origin: i128,
    width: Option<i128>,
}

impl Windows {
    pub(crate) fn new(range: TimeRange, width: Option<i64>) -> Self {
        let origin = range.start.map_or(0, |t| i128::from(t.as_nanos()));
        Self {
            range,
            origin,
            width: width.map(i128::from).filter(|w| *w > 0),
        }
    }

    /// The window holding `t`.
    pub(crate) fn index_of(&self, t: Timestamp) -> i64 {
        self.width.map_or(0, |w| {
            let k = (i128::from(t.as_nanos()) - self.origin).div_euclid(w);
            i64::try_from(k).unwrap_or(i64::MAX)
        })
    }

    /// The first instant of window `index`, as far as a timestamp can say.
    pub(crate) fn first_instant(&self, index: i64) -> Option<Timestamp> {
        let w = self.width?;
        let t = self.origin + i128::from(index) * w;
        i64::try_from(t).ok().map(Timestamp::from_nanos)
    }

    /// Window `index` as half-open bounds, clipped to the interval.
    pub(crate) fn bounds(&self, index: i64) -> (Option<Timestamp>, Option<Timestamp>) {
        let Some(w) = self.width else {
            return (self.range.start, self.range.end);
        };
        let start = self.origin + i128::from(index) * w;
        let end = start + w;
        let time = |t: i128| i64::try_from(t).ok().map(Timestamp::from_nanos);
        let clipped = match self.range.end {
            Some(limit) => Some(limit.min(time(end).unwrap_or(limit))),
            None => time(end),
        };
        (time(start), clipped)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(n: i64) -> Timestamp {
        Timestamp::from_nanos(n)
    }

    #[test]
    fn windows_align_to_the_start_and_the_last_is_clipped() {
        let range = TimeRange {
            start: Some(at(7)),
            end: Some(at(26)),
        };
        let w = Windows::new(range, Some(10));
        assert_eq!(
            (w.index_of(at(7)), w.index_of(at(16)), w.index_of(at(17))),
            (0, 0, 1)
        );
        assert_eq!(w.bounds(1), (Some(at(17)), Some(at(26))));
    }

    #[test]
    fn with_no_start_windows_align_to_the_epoch_and_may_be_negative() {
        let w = Windows::new(TimeRange::default(), Some(10));
        assert_eq!(w.index_of(at(-1)), -1);
        assert_eq!(w.bounds(-1), (Some(at(-10)), Some(at(0))));
    }

    #[test]
    fn no_width_is_one_window_over_the_interval() {
        let w = Windows::new(TimeRange::default(), None);
        assert_eq!(w.index_of(at(i64::MIN)), w.index_of(at(i64::MAX)));
        assert_eq!(w.bounds(0), (None, None));
    }
}
