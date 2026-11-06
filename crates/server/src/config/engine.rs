//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The file's engine sections, and building `norn_core::Config` from them.

use std::time::Duration;

use norn_core::{Config, Limits, MaintenanceConfig, MemoryBudgets, MetadataLimits, StorageConfig};
use serde::Deserialize;

use super::ConfigError;

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Memory {
    total_bytes: Option<u64>,
    ingest_bytes: Option<u64>,
    chunk_cache_bytes: Option<u64>,
    index_bytes: Option<u64>,
    query_bytes: Option<u64>,
    replay_bytes: Option<u64>,
    ipc_bytes: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Count {
    max_batch_samples: Option<usize>,
    max_streams: Option<usize>,
    max_entities: Option<usize>,
    max_signals: Option<usize>,
    max_query_page_samples: Option<usize>,
    max_replay_sessions: Option<usize>,
    max_subscriptions: Option<usize>,
    max_clients: Option<usize>,
    max_pending_batches: Option<usize>,
    replay_session_idle_timeout_ms: Option<u64>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Meta {
    max_entries: Option<usize>,
    max_key_bytes: Option<usize>,
    max_value_bytes: Option<usize>,
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Storage {
    max_segment_bytes: Option<u64>,
    max_segment_age_ms: Option<u64>,
    max_segment_open_time_ms: Option<u64>,
    group_commit_interval_ms: Option<u64>,
    max_chunk_age_ms: Option<u64>,
    durable_acks: Option<bool>,
    encryption: Option<String>,
    key_file: Option<String>,
    key_env: Option<String>,
}

impl Storage {
    /// The encryption settings, taken out: the engine's `Config` has none.
    pub(super) fn key_source(
        &mut self,
    ) -> Result<Option<super::encryption::KeySource>, ConfigError> {
        super::encryption::source(
            self.encryption.take().as_deref(),
            self.key_file.take().map(Into::into),
            self.key_env.take(),
        )
    }
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Upkeep {
    background: Option<bool>,
    compaction_interval_ms: Option<u64>,
    retention_interval_ms: Option<u64>,
    max_runs_per_pass: Option<usize>,
    compaction_window: Option<String>,
}

impl Upkeep {
    fn build(self) -> Result<MaintenanceConfig, ConfigError> {
        let mut out = MaintenanceConfig::default();
        let ms = |v: Option<u64>| v.map(Duration::from_millis);
        set(&mut out.background, self.background);
        set(
            &mut out.compaction_interval,
            ms(self.compaction_interval_ms),
        );
        set(&mut out.retention_interval, ms(self.retention_interval_ms));
        set(&mut out.max_runs_per_pass, self.max_runs_per_pass);
        if let Some(text) = self.compaction_window {
            let window = parse_window(&text).ok_or_else(|| {
                ConfigError::invalid("maintenance.compaction_window", "not HH:MM-HH:MM")
            })?;
            out.compaction_window = Some(window);
        }
        Ok(out)
    }
}

/// `HH:MM-HH:MM`, UTC, as minutes from midnight.
fn parse_window(text: &str) -> Option<(u16, u16)> {
    let minute = |t: &str| {
        let (h, m) = t.split_once(':')?;
        let (h, m): (u16, u16) = (h.parse().ok()?, m.parse().ok()?);
        (h < 24 && m < 60).then_some(h * 60 + m)
    };
    let (start, end) = text.split_once('-')?;
    Some((minute(start)?, minute(end)?))
}

/// Sets `field` to `value` when the file gave one.
pub(super) fn set<T>(field: &mut T, value: Option<T>) {
    if let Some(v) = value {
        *field = v;
    }
}

pub(super) fn engine(
    m: Option<Memory>,
    c: Option<Count>,
    d: Option<Meta>,
    s: Option<Storage>,
    u: Option<Upkeep>,
) -> Result<Config, ConfigError> {
    let (m, c, d) = (
        m.unwrap_or_default(),
        c.unwrap_or_default(),
        d.unwrap_or_default(),
    );
    let mut memory = MemoryBudgets::default();
    set(&mut memory.total_bytes, m.total_bytes);
    set(&mut memory.ingest_bytes, m.ingest_bytes);
    set(&mut memory.chunk_cache_bytes, m.chunk_cache_bytes);
    set(&mut memory.index_bytes, m.index_bytes);
    set(&mut memory.query_bytes, m.query_bytes);
    set(&mut memory.replay_bytes, m.replay_bytes);
    set(&mut memory.ipc_bytes, m.ipc_bytes);
    let mut limits = Limits::default();
    set(&mut limits.max_batch_samples, c.max_batch_samples);
    set(&mut limits.max_streams, c.max_streams);
    set(&mut limits.max_entities, c.max_entities);
    set(&mut limits.max_signals, c.max_signals);
    set(&mut limits.max_query_page_samples, c.max_query_page_samples);
    set(&mut limits.max_replay_sessions, c.max_replay_sessions);
    set(&mut limits.max_subscriptions, c.max_subscriptions);
    set(&mut limits.max_clients, c.max_clients);
    set(&mut limits.max_pending_batches, c.max_pending_batches);
    let idle = c.replay_session_idle_timeout_ms.map(Duration::from_millis);
    set(&mut limits.replay_session_idle_timeout, idle);
    let mut metadata = MetadataLimits::default();
    set(&mut metadata.max_entries, d.max_entries);
    set(&mut metadata.max_key_bytes, d.max_key_bytes);
    set(&mut metadata.max_value_bytes, d.max_value_bytes);
    let s = s.unwrap_or_default();
    let mut storage = StorageConfig::default();
    set(&mut storage.max_segment_bytes, s.max_segment_bytes);
    let ms = |v: Option<u64>| v.map(Duration::from_millis);
    set(&mut storage.max_segment_age, ms(s.max_segment_age_ms));
    let open = ms(s.max_segment_open_time_ms);
    set(&mut storage.max_segment_open_time, open);
    let interval = ms(s.group_commit_interval_ms);
    set(&mut storage.group_commit_interval, interval);
    set(&mut storage.max_chunk_age, ms(s.max_chunk_age_ms));
    set(&mut storage.durable_acks, s.durable_acks);
    Config::builder()
        .memory(memory)
        .storage(storage)
        .limits(limits)
        .metadata(metadata)
        .maintenance(u.unwrap_or_default().build()?)
        .build()
        .map_err(ConfigError::Engine)
}
