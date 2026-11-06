//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The parts of Norn the fake does not need to be: what a sink calls succeeds
//! and remembers nothing, and everything else is `UNIMPLEMENTED`; batch create
//! among it, so the sink takes its one-at-a-time path, as against a daemon
//! before protocol 1.6. Its negotiation publishes no message limit, as a daemon
//! before protocol 1.8 does not.

use norn_protocol::admin::admin_service_server::AdminService;
use norn_protocol::admin::{self, Limits, NegotiateProtocolRequest, NegotiateProtocolResponse};
use norn_protocol::entity::entity_service_server::EntityService;
use norn_protocol::entity::{self, Entity};
use norn_protocol::google::longrunning::Operation;
use norn_protocol::signal::signal_service_server::SignalService;
use norn_protocol::signal::{self, Signal};
use norn_protocol::stream::stream_service_server::StreamService;
use norn_protocol::stream::{self, Stream};
use tonic::{Request, Response, Status, async_trait};

/// What is not part of what is under test.
#[derive(Clone, Copy, Default)]
pub struct Stubs;

macro_rules! refusing {
    ($service:ident { $($name:ident($req:ty) -> $res:ty;)* } $($served:tt)*) => {
        #[async_trait]
        impl $service for Stubs {
            $(async fn $name(&self, _: Request<$req>) -> Result<Response<$res>, Status> {
                Err(Status::unimplemented(stringify!($name)))
            })*
            $($served)*
        }
    };
}

refusing! {
    AdminService { get_server(admin::GetServerRequest) -> admin::Server; }
    async fn negotiate_protocol(
        &self,
        _: Request<NegotiateProtocolRequest>,
    ) -> Result<Response<NegotiateProtocolResponse>, Status> {
        Ok(Response::new(NegotiateProtocolResponse {
            limits: Some(Limits {
                max_batch_samples: 2,
                max_batch_bytes: 1 << 20,
                max_pending_batches: 8,
                ..Limits::default()
            }),
            ..NegotiateProtocolResponse::default()
        }))
    }
}

refusing! {
    EntityService {
        get_entity(entity::GetEntityRequest) -> Entity;
        list_entities(entity::ListEntitiesRequest) -> entity::ListEntitiesResponse;
        update_entity(entity::UpdateEntityRequest) -> Entity;
        delete_entity(entity::DeleteEntityRequest) -> ();
        batch_create_entities(entity::BatchCreateEntitiesRequest)
            -> entity::BatchCreateEntitiesResponse;
    }
    async fn create_entity(
        &self,
        _: Request<entity::CreateEntityRequest>,
    ) -> Result<Response<Entity>, Status> {
        Ok(Response::new(Entity::default()))
    }
}

refusing! {
    SignalService {
        get_signal(signal::GetSignalRequest) -> Signal;
        list_signals(signal::ListSignalsRequest) -> signal::ListSignalsResponse;
        update_signal(signal::UpdateSignalRequest) -> Signal;
        delete_signal(signal::DeleteSignalRequest) -> ();
        batch_create_signals(signal::BatchCreateSignalsRequest)
            -> signal::BatchCreateSignalsResponse;
    }
    async fn create_signal(
        &self,
        _: Request<signal::CreateSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        Ok(Response::new(Signal::default()))
    }
}

refusing! {
    StreamService {
        get_stream(stream::GetStreamRequest) -> Stream;
        list_streams(stream::ListStreamsRequest) -> stream::ListStreamsResponse;
        update_stream(stream::UpdateStreamRequest) -> Stream;
        delete_stream(stream::DeleteStreamRequest) -> ();
        compact_stream(stream::CompactStreamRequest) -> Operation;
        export_stream(stream::ExportStreamRequest) -> Operation;
        verify_stream(stream::VerifyStreamRequest) -> Operation;
        apply_stream_retention(stream::ApplyStreamRetentionRequest) -> Operation;
        get_segment(stream::GetSegmentRequest) -> stream::Segment;
        list_segments(stream::ListSegmentsRequest) -> stream::ListSegmentsResponse;
    }
    async fn create_stream(
        &self,
        _: Request<stream::CreateStreamRequest>,
    ) -> Result<Response<Stream>, Status> {
        Err(Status::already_exists("from a past run"))
    }
}
