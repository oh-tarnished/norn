//! A process spawn must not prolong a stream writer's exclusive lock.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use super::{StorageError, StreamLock};

#[test]
fn dropping_the_stream_lock_unlocks_even_with_an_inherited_descriptor() {
    let dir = std::env::temp_dir().join(format!("norn-stream-lock-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lock = StreamLock::acquire(&dir).unwrap();
    let inherited = lock._file.try_clone().unwrap();
    assert!(matches!(
        StreamLock::acquire(&dir),
        Err(StorageError::Locked { .. })
    ));
    drop(lock);
    let reopened = StreamLock::acquire(&dir).expect("the writer released its lock");
    drop(inherited);
    assert!(matches!(
        StreamLock::acquire(&dir),
        Err(StorageError::Locked { .. })
    ));
    drop(reopened);
    std::fs::remove_dir_all(dir).unwrap();
}
