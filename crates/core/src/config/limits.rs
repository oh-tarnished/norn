//! Count limits that stop one client exhausting the daemon.

use std::time::Duration;

/// Upper bounds on counts (SPEC §16, §41): no unbounded batches, streams,
/// subscriptions, query pages or replay sessions.
///
/// `#[non_exhaustive]`: start from [`Default`] and set fields.
///
/// `Default` (picked, not yet measured): batches of 10,000 samples, 1,024
/// streams, 100,000 entities and 100,000 signals (six times SPEC §15's
/// 16,000-signal site), query pages of 10,000 samples, 16 replay sessions,
/// 256 subscriptions, 64 clients, 256 pending batches per append call and a
/// 10-minute replay session idle timeout.
///
/// Invariant (checked by [`crate::ConfigBuilder::build`]): every limit is
/// non-zero, because a zero limit makes its subsystem unusable. That includes
/// `replay_session_idle_timeout`, which would expire a session at birth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct Limits {
    /// Samples in one write batch.
    pub max_batch_samples: usize,
    /// Streams in one database.
    pub max_streams: usize,
    /// Entities in one database.
    pub max_entities: usize,
    /// Signals in one database.
    pub max_signals: usize,
    /// Samples in one page of query results; larger results are paginated.
    pub max_query_page_samples: usize,
    /// Concurrent replay sessions.
    pub max_replay_sessions: usize,
    /// Concurrent live or replay subscriptions.
    pub max_subscriptions: usize,
    /// Concurrently connected clients.
    pub max_clients: usize,
    /// Batches one `AppendSamples` call may have unacknowledged at the level
    /// it asked for. One past this is rejected whole, with a reason.
    pub max_pending_batches: usize,
    /// How long a replay session may sit idle before the daemon deletes it.
    /// Published now; enforced by the replay subsystem when it exists.
    pub replay_session_idle_timeout: Duration,
}

impl Limits {
    /// Every count limit, by field name, for validation and reports.
    #[must_use]
    pub fn all(&self) -> [(&'static str, usize); 9] {
        [
            ("max_batch_samples", self.max_batch_samples),
            ("max_streams", self.max_streams),
            ("max_entities", self.max_entities),
            ("max_signals", self.max_signals),
            ("max_query_page_samples", self.max_query_page_samples),
            ("max_replay_sessions", self.max_replay_sessions),
            ("max_subscriptions", self.max_subscriptions),
            ("max_clients", self.max_clients),
            ("max_pending_batches", self.max_pending_batches),
        ]
    }
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_batch_samples: 10_000,
            max_streams: 1_024,
            max_entities: 100_000,
            max_signals: 100_000,
            max_query_page_samples: 10_000,
            max_replay_sessions: 16,
            max_subscriptions: 256,
            max_clients: 64,
            max_pending_batches: 256,
            replay_session_idle_timeout: Duration::from_secs(600),
        }
    }
}
