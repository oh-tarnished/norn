//! Shutdown must wait for the actual lock owners, including cancelled calls.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::convert::Infallible;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_core::Config;
use norn_metadata::{MetadataError, Registry};
use tokio_util::task::TaskTracker;
use tower::{Layer, Service};

use super::DrainLayer;

#[derive(Clone)]
struct Holding(Arc<Registry>);

impl Service<()> for Holding {
    type Response = ();
    type Error = Infallible;
    type Future = Pin<Box<dyn Future<Output = Result<(), Infallible>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Infallible>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, _: ()) -> Self::Future {
        let registry = Arc::clone(&self.0);
        Box::pin(async move {
            let _registry = registry;
            std::future::pending().await
        })
    }
}

#[tokio::test]
async fn shutdown_waits_for_service_clones_and_cancelled_calls_to_release_the_lock() {
    let dir = std::env::temp_dir().join(format!("norn-drain-{}", std::process::id()));
    let config = Config::default();
    let open = || Registry::open(&dir, config.limits(), config.metadata());
    let registry = Arc::new(open().unwrap().0);
    let tracker = TaskTracker::new();
    let service = DrainLayer(tracker.clone()).layer(Holding(registry));
    let mut cloned = service.clone();
    drop(service);
    tracker.close();
    assert!(
        !tracker.is_empty(),
        "a service clone still owns the registry"
    );
    assert!(matches!(open(), Err(MetadataError::Locked { .. })));

    let call = cloned.call(());
    drop(cloned);
    assert!(
        !tracker.is_empty(),
        "the request future still owns the registry"
    );
    assert!(matches!(open(), Err(MetadataError::Locked { .. })));

    drop(call);
    tracker.wait().await;
    drop(open().expect("a completed drain must release the database lock"));
    std::fs::remove_dir_all(dir).unwrap();
}
