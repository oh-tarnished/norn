//! SPEC §41 protections on Unix sockets.
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::fs;
use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
use std::os::unix::net::UnixListener as StdUnixListener;
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_transport::{Endpoint, TransportError, bind, connect};

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A fresh, not-yet-created directory path, short enough for `sun_path`.
fn scratch() -> PathBuf {
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("nt{}-{n}", std::process::id()))
}

/// A fresh owner-only directory, created.
fn owner_only_dir() -> PathBuf {
    let dir = scratch();
    fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
    dir
}

fn mode(path: &std::path::Path) -> u32 {
    fs::metadata(path).unwrap().permissions().mode() & 0o777
}

#[tokio::test]
async fn the_socket_is_0600_inside_a_created_0700_directory() {
    let dir = scratch();
    let path = dir.join("n.sock");
    let listener = bind(&Endpoint::Unix(path.clone())).await.unwrap();
    assert_eq!(mode(&dir), 0o700);
    assert_eq!(mode(&path), 0o600);
    drop(listener);
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_stale_socket_is_replaced() {
    let dir = owner_only_dir();
    let path = dir.join("n.sock");
    // A std listener leaves its file behind when dropped: a crashed daemon.
    drop(StdUnixListener::bind(&path).unwrap());
    assert!(path.exists());
    let mut listener = bind(&Endpoint::Unix(path.clone())).await.unwrap();
    let client = connect(&Endpoint::Unix(path.clone())).await.unwrap();
    listener.accept().await.unwrap();
    drop(client);
    drop(listener);
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_live_socket_is_refused_and_left_running() {
    let dir = owner_only_dir();
    let path = dir.join("n.sock");
    let live = StdUnixListener::bind(&path).unwrap();
    let err = bind(&Endpoint::Unix(path.clone())).await.err().unwrap();
    assert!(matches!(err, TransportError::LiveSocket(p) if p == path));
    assert!(std::os::unix::net::UnixStream::connect(&path).is_ok());
    drop(live);
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_regular_file_at_the_path_is_refused_and_kept() {
    let dir = owner_only_dir();
    let path = dir.join("n.sock");
    fs::write(&path, b"not a socket").unwrap();
    let err = bind(&Endpoint::Unix(path.clone())).await.err().unwrap();
    assert!(matches!(err, TransportError::NotASocket(_)));
    assert_eq!(fs::read(&path).unwrap(), b"not a socket");
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_directory_open_to_others_is_refused() {
    let dir = owner_only_dir();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    let err = bind(&Endpoint::Unix(dir.join("n.sock")))
        .await
        .err()
        .unwrap();
    assert!(matches!(
        err,
        TransportError::InsecureDirectory { mode: 0o755, .. }
    ));
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn the_socket_file_is_removed_on_drop() {
    let dir = owner_only_dir();
    let path = dir.join("n.sock");
    drop(bind(&Endpoint::Unix(path.clone())).await.unwrap());
    assert!(!path.exists());
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_socket_that_replaced_ours_survives_our_drop() {
    let dir = owner_only_dir();
    let path = dir.join("n.sock");
    let ours = bind(&Endpoint::Unix(path.clone())).await.unwrap();
    fs::remove_file(&path).unwrap();
    let theirs = StdUnixListener::bind(&path).unwrap();
    drop(ours);
    assert!(path.exists(), "another server's socket was deleted");
    drop(theirs);
    fs::remove_file(&path).unwrap();
    fs::remove_dir(&dir).unwrap();
}

#[tokio::test]
async fn a_named_pipe_is_unsupported_on_unix() {
    let ep: Endpoint = r"pipe:\\.\pipe\norn".parse().unwrap();
    let err = bind(&ep).await.err().unwrap();
    assert!(matches!(err, TransportError::Unsupported(_)));
}

#[test]
fn the_default_endpoint_is_a_unix_socket() {
    assert_eq!(Endpoint::platform_default().scheme(), "unix");
}
