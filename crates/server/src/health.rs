//! Reflecting the runtime's health into `grpc.health.v1`.

use norn_protocol::admin::admin_service_server::SERVICE_NAME as ADMIN;
use norn_runtime::{Health, HealthWatch, TaskError};
use tokio_util::sync::CancellationToken;
use tonic_health::ServingStatus;
use tonic_health::server::HealthReporter;

/// The health service names the daemon reports: `""` (the whole daemon,
/// what `nornctl status` and most probes ask about) and AdminService.
const SERVICES: [&str; 2] = ["", ADMIN];

/// The gRPC status for a runtime state: `SERVING` only while serving, so a
/// load balancer or supervisor stops sending work the moment draining
/// begins.
#[must_use]
pub fn serving_status(health: Health) -> ServingStatus {
    if health.is_serving() {
        ServingStatus::Serving
    } else {
        ServingStatus::NotServing
    }
}

/// Sets every reported service to `status`.
pub(crate) async fn report(reporter: &HealthReporter, status: ServingStatus) {
    for name in SERVICES {
        reporter.set_service_status(name, status).await;
    }
}

/// A runtime task: mirrors `watch` into `reporter` until the runtime stops
/// or `token` is cancelled, then reports `NOT_SERVING` and returns. Holds
/// nothing but the two handles, so it stops within one poll of either.
pub(crate) async fn reflect(
    mut watch: HealthWatch,
    reporter: HealthReporter,
    token: CancellationToken,
) -> Result<(), TaskError> {
    let mut current = watch.get();
    loop {
        report(&reporter, serving_status(current)).await;
        if current == Health::Stopped {
            return Ok(());
        }
        tokio::select! {
            () = token.cancelled() => {
                report(&reporter, ServingStatus::NotServing).await;
                return Ok(());
            }
            next = watch.changed() => current = next,
        }
    }
}
