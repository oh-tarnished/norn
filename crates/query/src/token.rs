//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Where a query has got to, as text a client can hand back.

use norn_core::Timestamp;

/// The last result delivered. A resumed query continues just after it, with
/// `Position`s comparing by the query's own order.
///
/// A position names a signal rather than an index into the selection, so it
/// stays meaningful when signals are created between the two calls, and a sample
/// by its time and sequence rather than by a chunk, since compaction moves chunks.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum Position {
    /// After this sample of `signal`.
    Sample {
        /// The signal's resource name.
        signal: String,
        /// The sample's time.
        time: Timestamp,
        /// The sample's sequence.
        sequence: u64,
    },
    /// After this window of `signal`.
    Window {
        /// The signal's resource name.
        signal: String,
        /// The window's index from the query's origin. May be negative when the
        /// query has no start and windows align to the Unix epoch.
        index: i64,
    },
}

impl Position {
    /// This position as text. Round-trips through [`Position::parse`].
    #[must_use]
    pub fn encode(&self) -> String {
        match self {
            Self::Sample {
                signal,
                time,
                sequence,
            } => format!("s.{}.{sequence}.{signal}", time.as_nanos()),
            Self::Window { signal, index } => format!("w.{index}.{signal}"),
        }
    }

    /// Parses [`Position::encode`]'s output; `None` for anything else. Borrows
    /// nothing from `text`.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (kind, rest) = text.split_once('.')?;
        match kind {
            "s" => {
                let mut parts = rest.splitn(3, '.');
                let time = Timestamp::from_nanos(parts.next()?.parse().ok()?);
                let sequence = parts.next()?.parse().ok()?;
                let signal = parts.next().filter(|s| !s.is_empty())?.to_owned();
                Some(Self::Sample {
                    signal,
                    time,
                    sequence,
                })
            }
            "w" => {
                let (index, signal) = rest.split_once('.')?;
                let index = index.parse().ok()?;
                let signal = Some(signal).filter(|s| !s.is_empty())?.to_owned();
                Some(Self::Window { signal, index })
            }
            _ => None,
        }
    }

    /// The signal it names.
    pub(crate) fn signal(&self) -> &str {
        match self {
            Self::Sample { signal, .. } | Self::Window { signal, .. } => signal,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_position_round_trips_with_dots_and_a_negative_time_in_it() {
        let cases = [
            Position::Sample {
                signal: "entities/a.b/signals/c.d".into(),
                time: Timestamp::from_nanos(-5),
                sequence: u64::MAX,
            },
            Position::Window {
                signal: "entities/a/signals/b".into(),
                index: -3,
            },
        ];
        for p in cases {
            assert_eq!(Position::parse(&p.encode()), Some(p));
        }
    }

    #[test]
    fn nonsense_is_refused_rather_than_misread() {
        for text in ["", "x", "s.1.2.", "s.a.2.n", "w.1.", "q.1.2.n"] {
            assert_eq!(Position::parse(text), None, "{text}");
        }
    }
}
