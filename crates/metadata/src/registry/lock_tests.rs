//! A duplicated Unix descriptor must not extend the registry's lock lifetime.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_core::Config;

use super::Registry;

#[test]
fn dropping_the_registry_unlocks_even_while_an_inherited_descriptor_remains_open() {
    let dir = std::env::temp_dir().join(format!("norn-registry-lock-{}", std::process::id()));
    let config = Config::default();
    let open = || Registry::open(&dir, config.limits(), config.metadata());
    let registry = open().unwrap().0;
    // A concurrent process spawn can inherit this open file description until
    // exec closes CLOEXEC descriptors. A duplicate models that deterministically.
    let inherited = registry._lock.try_clone().unwrap();
    drop(registry);
    let reopened = open().expect("the registry owner has released its lock").0;
    drop(inherited);
    assert!(matches!(open(), Err(crate::MetadataError::Locked { .. })));
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
