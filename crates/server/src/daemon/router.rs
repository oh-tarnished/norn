//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Opening the database and assembling the services.

use std::sync::Arc;

use norn_protocol::admin::admin_service_server::AdminServiceServer;
use norn_protocol::entity::entity_service_server::EntityServiceServer;
use norn_protocol::google::longrunning::operations_server::OperationsServer;
use norn_protocol::query::query_service_server::QueryServiceServer;
use norn_protocol::replay::replay_service_server::ReplayServiceServer;
use norn_protocol::signal::signal_service_server::SignalServiceServer;
use norn_protocol::state::state_service_server::StateServiceServer;
use norn_protocol::stream::stream_service_server::StreamServiceServer;
use norn_protocol::telemetry::telemetry_service_server::TelemetryServiceServer;
use norn_runtime::maintenance::Maintenance;
use norn_runtime::sessions::Replays;
use norn_runtime::{Reservation, Runtime, RuntimeError, Subsystem};
use tokio_util::task::TaskTracker;
use tonic_health::ServingStatus;
use tonic_health::server::HealthReporter;
use tpp_grpc::HybridServer;

use super::DaemonError;
use super::open::Database;
use crate::admin::Admin;
use crate::catalog::Catalog;
use crate::config::DaemonConfig;
use crate::events::Subscriptions;
use crate::health::{health_server, report};
use crate::info::{MAX_MESSAGE_BYTES, connection_window, max_batch_bytes, server_resource};
use crate::metered::{MeterLayer, MeterLimits};
use crate::operations::OperationsService;
use crate::query::Queries;
use crate::replay::ReplaySessions;
use crate::state::States;
use crate::telemetry::{Plane, Telemetry};

/// What `build_router` makes, for the daemon to hold and run.
pub(super) struct Built {
    pub(super) server: HybridServer,
    /// The flow-control reservation, held for life.
    pub(super) windows: Reservation,
    pub(super) reporter: HealthReporter,
    pub(super) replays: Arc<Replays>,
    pub(super) maintenance: Arc<Maintenance>,
    /// The tasks calls spawn: shutdown closes it and waits, before ingestion
    /// closes, so no call outlives the database it holds.
    pub(super) calls: TaskTracker,
}

/// Builds every service. Returns the flow-control reservation, held for life.
pub(super) async fn build_router(
    config: &DaemonConfig,
    runtime: &Runtime,
    database: Database,
    wiring: super::serve::Wiring,
) -> Result<Built, DaemonError> {
    let engine = config.engine();
    let Database {
        registry,
        ingest,
        recovery,
    } = database;
    let (reporter, health) = health_server(ingest.draining());
    report(&reporter, ServingStatus::NotServing).await;
    let resource = server_resource(engine, runtime.start_time(), &recovery);
    let upkeep_memory = runtime.budgets().get(Subsystem::ChunkCache).clone();
    let clock = Arc::clone(runtime.clock());
    let maintenance = Arc::new(Maintenance::new(
        Arc::clone(&ingest),
        Arc::clone(&registry),
        upkeep_memory,
        clock,
        engine.maintenance(),
    ));
    let clock = Arc::clone(runtime.clock());
    let (r, i, m) = (&registry, &ingest, &maintenance);
    let catalog = Catalog::new(Arc::clone(r), Arc::clone(i), clock, Arc::clone(m));
    let reads = runtime.budgets().get(Subsystem::Query).clone();
    let reading = ingest
        .query_engine(reads, engine)
        .ok_or(DaemonError::IndexMissing)?;
    let calls = TaskTracker::new();
    let queries = Queries::new(Arc::clone(&registry), Arc::clone(&reading)).tracked(calls.clone());
    let clock = Arc::clone(runtime.clock());
    let states = States::new(Arc::clone(&registry), Arc::clone(&reading), clock);
    let replay_budget = runtime.budgets().get(Subsystem::Replay).clone();
    ingest.live().charge_to(replay_budget.clone());
    let replaying = ingest
        .replay_engine(replay_budget, engine)
        .ok_or(DaemonError::IndexMissing)?;
    let clock = Arc::clone(runtime.clock());
    let replays = Arc::new(Replays::new(replaying, engine, Arc::clone(&clock)));
    let sessions = ReplaySessions::new(Arc::clone(&registry), Arc::clone(&replays), reading);
    let limit = engine.limits().max_subscriptions;
    let shared = (
        Arc::clone(&registry),
        Arc::clone(&ingest),
        Arc::clone(&replays),
    );
    let events =
        Subscriptions::new(shared.0, shared.1, shared.2, clock, limit).tracked(calls.clone());
    let ipc = runtime.budgets().get(Subsystem::Ipc).clone();
    let windows = ipc.try_reserve(ipc.capacity() / 2).map_err(|e| {
        DaemonError::Runtime(RuntimeError::TaskFailed {
            task: "bind",
            source: Box::new(e),
        })
    })?;
    let limits = MeterLimits {
        max_batch_bytes: max_batch_bytes(engine),
        max_batch_samples: engine.limits().max_batch_samples,
    };
    let (endpoint, settings) = (config.endpoint(), config.shared_memory_settings());
    let plane = Plane::new(settings, &endpoint, ipc.clone(), limits, &calls);
    let admin = Admin::new(resource).maintenance(Arc::clone(&maintenance));
    let admin = admin.shared_memory(plane.as_ref().map(|p| p.advertised()));
    let admin = AdminServiceServer::new(admin).max_decoding_message_size(MAX_MESSAGE_BYTES);
    let telemetry = Telemetry::new(registry, ingest, engine)
        .events(Arc::new(events))
        .tracked(calls.clone())
        .plane(plane);
    let meter = MeterLayer::new(ipc, limits);
    let window = connection_window(engine.memory().ipc_bytes / 2, engine.limits().max_clients);
    let services = super::serve::Registered {
        health,
        admin,
        entities: EntityServiceServer::new(catalog.clone()),
        signals: SignalServiceServer::new(catalog.clone()),
        streams: StreamServiceServer::new(catalog),
        telemetry: TelemetryServiceServer::new(telemetry),
        queries: QueryServiceServer::new(queries),
        replays: ReplayServiceServer::new(sessions),
        states: StateServiceServer::new(states),
        operations: OperationsServer::new(OperationsService::new(Arc::clone(&maintenance))),
    };
    let server = super::serve::build(config, wiring, services, meter, window)?;
    Ok(Built {
        server,
        windows,
        reporter,
        replays,
        maintenance,
        calls,
    })
}
