//! The `Server` resource: what this daemon is, offers and enforces.

use norn_core::{Config, Timestamp};
use norn_protocol::PROTOCOL_VERSION;
use norn_protocol::admin::{Capability, IndexReport, Limits, RecoveryReport, Server};
use norn_runtime::ingest::IngestRecovery;

/// The daemon's release, from its Cargo package version.
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The largest gRPC message the daemon decodes, in bytes; larger requests
/// fail with `RESOURCE_EXHAUSTED` before any handler runs. It caps
/// [`max_batch_bytes`]. 4 MiB is tonic's default.
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;

/// The capabilities a daemon with `config` offers. `ACK_DURABLE` needs a
/// durability boundary, so `durable_acks = false` withdraws it.
#[must_use]
pub fn capabilities(config: &Config) -> Vec<Capability> {
    let mut offered = vec![
        Capability::AppendSamples,
        Capability::AckPersisted,
        Capability::QueryRange,
        Capability::QueryAggregates,
        Capability::StateAt,
        Capability::LiveSubscription,
        Capability::Replay,
    ];
    if config.storage().durable_acks {
        offered.push(Capability::AckDurable);
    }
    offered
}

/// The `Server` resource for a daemon with `config`, started at `start`, whose
/// startup recovery found `recovery`.
#[must_use]
pub fn server_resource(config: &Config, start: Timestamp, recovery: &IngestRecovery) -> Server {
    Server {
        name: "server".to_owned(),
        server_version: SERVER_VERSION.to_owned(),
        protocol_version: Some(PROTOCOL_VERSION.into()),
        capabilities: capabilities(config).iter().map(|c| *c as i32).collect(),
        limits: Some(limits(config)),
        start_time: Some(timestamp(start)),
        recovery: Some(RecoveryReport {
            stream_count: i32::try_from(recovery.streams).unwrap_or(i32::MAX),
            event_count: wide(recovery.events),
            corruption_count: wide(recovery.corruptions),
            replayed_batch_count: wide(recovery.replayed_batches),
            replayed_sample_count: wide(recovery.replayed_samples),
            duplicate_sample_count: wide(recovery.duplicate_samples),
            findings: recovery.findings.clone(),
        }),
        index: Some(index_report(recovery)),
    }
}

fn index_report(recovery: &IngestRecovery) -> IndexReport {
    let i = recovery.index;
    let narrow = |n: u64| i32::try_from(n).unwrap_or(i32::MAX);
    IndexReport {
        stream_count: narrow(i.streams),
        rebuilt_stream_count: narrow(i.rebuilt_streams),
        reused_segment_count: wide(i.reused_segments),
        read_segment_count: wide(i.read_segments),
        tail_segment_count: wide(i.tail_segments),
        unpersisted_stream_count: narrow(i.unpersisted_streams),
    }
}

fn wide(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

/// The largest batch: 4 MiB, or less if decoding one at full expansion would
/// not fit the part of `ipc_bytes` set aside for decoding, an even half.
#[must_use]
pub fn max_batch_bytes(config: &Config) -> usize {
    let quarter_of_pool = config.memory().ipc_bytes / 8;
    usize::try_from(quarter_of_pool).map_or(MAX_MESSAGE_BYTES, |n| n.min(MAX_MESSAGE_BYTES))
}

/// The published limits; every one is enforced.
fn limits(config: &Config) -> Limits {
    let l = config.limits();
    let count = |n: usize| i32::try_from(n).unwrap_or(i32::MAX);
    let idle = i64::try_from(l.replay_session_idle_timeout.as_secs()).unwrap_or(i64::MAX);
    Limits {
        max_batch_samples: count(l.max_batch_samples),
        max_batch_bytes: i64::try_from(max_batch_bytes(config)).unwrap_or(i64::MAX),
        max_pending_batches: count(l.max_pending_batches),
        max_streams: count(l.max_streams),
        max_replay_sessions: count(l.max_replay_sessions),
        max_subscriptions: count(l.max_subscriptions),
        replay_session_idle_timeout: Some(prost_types::Duration {
            seconds: idle,
            nanos: 0,
        }),
    }
}

/// A norn-core timestamp as `google.protobuf.Timestamp`, whose `nanos` is
/// always non-negative, even before the epoch.
pub(crate) fn timestamp(t: Timestamp) -> prost_types::Timestamp {
    let nanos = t.as_nanos();
    prost_types::Timestamp {
        seconds: nanos.div_euclid(1_000_000_000),
        nanos: i32::try_from(nanos.rem_euclid(1_000_000_000)).unwrap_or(0),
    }
}

/// HTTP/2's smallest flow-control window (RFC 9113 §6.9.2).
const MIN_WINDOW: u64 = 65_535;

/// The inbound window per connection: an equal share of the IPC budget, so
/// a full house of clients cannot buffer more than the budget in flow
/// control. Floored at HTTP/2's minimum, so a tiny budget stays usable.
pub(crate) fn connection_window(ipc_bytes: u64, max_clients: usize) -> u32 {
    let clients = u64::try_from(max_clients).unwrap_or(u64::MAX).max(1);
    let share = (ipc_bytes / clients).max(MIN_WINDOW);
    u32::try_from(share).unwrap_or(u32::MAX)
}

/// The inbound window per call, a `calls + 1`th of the connection's. A call
/// waiting for decode budget stops reading, and its unread bytes stay charged to
/// the connection; this leaves room, however many wait, for the one call holding
/// the budget to finish its message, without which they would deadlock.
pub(crate) fn stream_window(connection: u32, calls: u32) -> u32 {
    (connection / calls.saturating_add(1)).max(1)
}

#[cfg(test)]
mod tests {
    use super::{connection_window, stream_window};

    #[test]
    fn calls_blocked_at_their_stream_windows_cannot_fill_the_connection() {
        let (connection, calls) = (65_535, 16);
        assert!(stream_window(connection, calls) * calls < connection);
    }

    #[test]
    fn each_connection_gets_an_equal_share_of_the_ipc_budget() {
        assert_eq!(connection_window(32 << 20, 64), 512 << 10);
    }

    #[test]
    fn a_tiny_budget_still_leaves_the_smallest_legal_window() {
        assert_eq!(connection_window(1024, 64), 65_535);
    }

    #[test]
    fn a_huge_budget_saturates_at_what_http2_can_express() {
        assert_eq!(connection_window(u64::MAX, 1), u32::MAX);
    }
}
