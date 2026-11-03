//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Engine configuration: explicit memory budgets and count limits.
//!
//! A [`Config`] exists only once validated: the sole way to make one is
//! [`Config::default`] (valid by construction, proven by a test) or
//! [`ConfigBuilder::build`]. Loading it from a file is the daemon's job; the
//! core performs no I/O.

mod limits;
mod maintenance;
mod memory;
mod storage;

pub use limits::Limits;
pub use maintenance::MaintenanceConfig;
pub use memory::MemoryBudgets;
pub use storage::StorageConfig;

use crate::error::{ConfigError, ConfigField, Error, Result};
use crate::metadata::MetadataLimits;

/// Validated engine configuration.
///
/// Concurrency: `Copy` plain data; share it freely.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Config {
    memory: MemoryBudgets,
    limits: Limits,
    metadata: MetadataLimits,
    storage: StorageConfig,
    maintenance: MaintenanceConfig,
}

impl Config {
    /// A builder starting from the defaults.
    pub fn builder() -> ConfigBuilder {
        ConfigBuilder::default()
    }

    /// The memory budgets.
    #[must_use]
    pub fn memory(&self) -> MemoryBudgets {
        self.memory
    }

    /// The count limits.
    #[must_use]
    pub fn limits(&self) -> Limits {
        self.limits
    }

    /// The bounds every descriptor's metadata map enforces.
    #[must_use]
    pub fn metadata(&self) -> MetadataLimits {
        self.metadata
    }

    /// How segments roll and how durability is grouped.
    #[must_use]
    pub fn storage(&self) -> StorageConfig {
        self.storage
    }

    /// When compaction and retention run.
    #[must_use]
    pub fn maintenance(&self) -> MaintenanceConfig {
        self.maintenance
    }
}

/// Builds a [`Config`], validating it as a whole on [`ConfigBuilder::build`].
///
/// Every setter is optional; unset parts keep their documented defaults, so
/// adding an option never breaks a caller.
#[derive(Debug, Clone, Default)]
#[must_use]
pub struct ConfigBuilder {
    config: Config,
}

impl ConfigBuilder {
    /// Replaces the memory budgets.
    pub fn memory(mut self, memory: MemoryBudgets) -> Self {
        self.config.memory = memory;
        self
    }

    /// Replaces the count limits.
    pub fn limits(mut self, limits: Limits) -> Self {
        self.config.limits = limits;
        self
    }

    /// Replaces the metadata bounds.
    pub fn metadata(mut self, metadata: MetadataLimits) -> Self {
        self.config.metadata = metadata;
        self
    }

    /// Replaces the storage settings.
    pub fn storage(mut self, storage: StorageConfig) -> Self {
        self.config.storage = storage;
        self
    }

    /// Replaces the maintenance schedule.
    pub fn maintenance(mut self, maintenance: MaintenanceConfig) -> Self {
        self.config.maintenance = maintenance;
        self
    }

    /// Validates and returns the configuration.
    ///
    /// # Errors
    ///
    /// [`Error::InvalidConfig`] naming the first offending field: a zero
    /// budget, limit or storage setting ([`ConfigError::Zero`]), or subsystem
    /// budgets that sum past the total ([`ConfigError::ExceedsTotal`]).
    pub fn build(self) -> Result<Config> {
        let c = self.config;
        let invalid = |field, reason| Err(Error::InvalidConfig { field, reason });
        if c.memory.total_bytes == 0 {
            return invalid(ConfigField::MemoryTotal, ConfigError::Zero);
        }
        let mut sum: u64 = 0;
        for (name, bytes) in c.memory.subsystems() {
            if bytes == 0 {
                return invalid(ConfigField::Named(name), ConfigError::Zero);
            }
            sum = sum.saturating_add(bytes);
        }
        if sum > c.memory.total_bytes {
            return invalid(
                ConfigField::MemorySubsystems,
                ConfigError::ExceedsTotal(sum),
            );
        }
        let metadata = [
            ("max_entries", c.metadata.max_entries),
            ("max_key_bytes", c.metadata.max_key_bytes),
            ("max_value_bytes", c.metadata.max_value_bytes),
        ];
        for (name, n) in c.limits.all().into_iter().chain(metadata) {
            if n == 0 {
                return invalid(ConfigField::Named(name), ConfigError::Zero);
            }
        }
        if c.limits.replay_session_idle_timeout.is_zero() {
            let field = ConfigField::Named("replay_session_idle_timeout");
            return invalid(field, ConfigError::Zero);
        }
        if let Some(name) = c.storage.first_zero().or(c.maintenance.first_invalid()) {
            return invalid(ConfigField::Named(name), ConfigError::Zero);
        }
        Ok(c)
    }
}
