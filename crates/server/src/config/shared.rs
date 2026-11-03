//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `[shared_memory]`: the iceoryx2 data plane, off unless asked for
//! (docs/reference/shared-memory.md). The socket stays the control plane, and the
//! fallback whenever a ring is not granted.

use norn_core::Config;
use norn_shared_memory::{Settings, ring_bytes};
use serde::Deserialize;

use super::{ConfigError, DaemonConfig};
use crate::info::max_batch_bytes;

/// The fewest bytes a frame may hold: below this every batch would go by gRPC.
const MIN_FRAME_BYTES: usize = 4 * 1024;
/// The most frames a ring may hold.
const MAX_SLOTS: usize = 1024;
/// The most rings at once: each is a reader thread.
const MAX_RINGS: usize = 64;
/// The longest service name; the daemon adds its process id and a ring number.
const MAX_SERVICE: usize = 64;

/// The `[shared_memory]` table as TOML spells it.
#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SharedMemory {
    enabled: Option<bool>,
    service: Option<String>,
    max_rings: Option<usize>,
    slot_count: Option<usize>,
    max_frame_bytes: Option<usize>,
}

impl SharedMemory {
    /// The table over the defaults.
    pub(super) fn settings(self) -> Settings {
        let d = Settings::default();
        Settings {
            enabled: self.enabled.unwrap_or(d.enabled),
            service: self.service.unwrap_or(d.service),
            max_rings: self.max_rings.unwrap_or(d.max_rings),
            slot_count: self.slot_count.unwrap_or(d.slot_count),
            max_frame_bytes: self.max_frame_bytes.unwrap_or(d.max_frame_bytes),
        }
    }
}

fn plain(service: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.');
    !service.is_empty() && service.len() <= MAX_SERVICE && service.chars().all(allowed)
}

/// Refuses rings that would not be bounded, or could not be paid for: the half
/// of `ipc_bytes` not promised to flow-control windows must hold every ring and
/// leave a frame's worth for decoding. A frame may not exceed the largest batch
/// the socket takes, so neither plane admits what the other refuses. Settings
/// that are off are not looked at.
pub(super) fn check(settings: &Settings, engine: &Config) -> Result<(), ConfigError> {
    if !settings.enabled {
        return Ok(());
    }
    let bad = |key, detail: String| Err(ConfigError::invalid(key, detail));
    if !plain(&settings.service) {
        let detail = format!("1 to {MAX_SERVICE} letters, digits, '-', '_' or '.'");
        return bad("shared_memory.service", detail);
    }
    if settings.max_rings == 0 || settings.max_rings > MAX_RINGS {
        return bad(
            "shared_memory.max_rings",
            format!("must be 1 to {MAX_RINGS}"),
        );
    }
    if settings.slot_count == 0 || settings.slot_count > MAX_SLOTS {
        let detail = format!("must be 1 to {MAX_SLOTS}");
        return bad("shared_memory.slot_count", detail);
    }
    let largest = max_batch_bytes(engine);
    if settings.max_frame_bytes < MIN_FRAME_BYTES || settings.max_frame_bytes > largest {
        let detail = format!("must be {MIN_FRAME_BYTES} to {largest}, the largest batch");
        return bad("shared_memory.max_frame_bytes", detail);
    }
    let rings = ring_bytes(settings).saturating_mul(settings.max_rings as u64);
    let room = (engine.memory().ipc_bytes / 2).saturating_sub(settings.max_frame_bytes as u64);
    if rings > room {
        let detail = format!("{rings} bytes of rings exceed the {room} ipc_bytes leaves them");
        return bad("shared_memory", detail);
    }
    Ok(())
}

impl DaemonConfig {
    /// Replaces the shared-memory settings, checking them when they are on.
    ///
    /// # Errors
    ///
    /// [`ConfigError::Invalid`] for a bad name or limit, or rings that do not fit
    /// `ipc_bytes`.
    pub fn shared_memory(mut self, settings: Settings) -> Result<Self, ConfigError> {
        check(&settings, &self.engine)?;
        self.shared_memory = settings;
        Ok(self)
    }

    /// The shared-memory settings: off unless the file or
    /// [`shared_memory`](Self::shared_memory) turned them on.
    #[must_use]
    pub fn shared_memory_settings(&self) -> &Settings {
        &self.shared_memory
    }
}
