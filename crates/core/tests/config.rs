//! Configuration validation.

use norn_core::{Config, ConfigError, ConfigField, Error, Limits, MemoryBudgets, MetadataLimits};

#[test]
fn the_default_configuration_is_valid() {
    assert_eq!(Config::builder().build(), Ok(Config::default()));
}

#[test]
fn subsystem_budgets_may_not_sum_past_the_total() {
    let mut memory = MemoryBudgets::default();
    memory.total_bytes = 1024;
    let err = Config::builder().memory(memory).build().unwrap_err();
    assert!(matches!(
        err,
        Error::InvalidConfig {
            field: ConfigField::MemorySubsystems,
            reason: ConfigError::ExceedsTotal(_)
        }
    ));
}

#[test]
fn a_zero_budget_names_its_field() {
    let mut memory = MemoryBudgets::default();
    memory.replay_bytes = 0;
    assert_eq!(
        Config::builder().memory(memory).build(),
        Err(Error::InvalidConfig {
            field: ConfigField::Named("replay_bytes"),
            reason: ConfigError::Zero
        })
    );
    memory = MemoryBudgets::default();
    memory.total_bytes = 0;
    assert!(matches!(
        Config::builder().memory(memory).build(),
        Err(Error::InvalidConfig {
            field: ConfigField::MemoryTotal,
            ..
        })
    ));
}

#[test]
fn a_zero_limit_names_its_field() {
    let mut limits = Limits::default();
    limits.max_clients = 0;
    assert_eq!(
        Config::builder().limits(limits).build(),
        Err(Error::InvalidConfig {
            field: ConfigField::Named("max_clients"),
            reason: ConfigError::Zero
        })
    );
    let metadata = MetadataLimits {
        max_entries: 0,
        ..MetadataLimits::default()
    };
    assert!(Config::builder().metadata(metadata).build().is_err());
}

#[test]
fn a_valid_override_is_kept() {
    let mut limits = Limits::default();
    limits.max_signals = 16_000;
    let config = Config::builder().limits(limits).build().unwrap();
    assert_eq!(config.limits().max_signals, 16_000);
    assert_eq!(config.memory(), MemoryBudgets::default());
}
