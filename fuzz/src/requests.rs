//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Every request message of the API, decoded by a number.

use norn_protocol::{admin, entity, query, replay, signal, state, stream, telemetry};
use prost::Message;

/// Declares [`COUNT`] and [`decode`] over the listed request messages.
macro_rules! requests {
    ($($request:ty),+ $(,)?) => {
        /// How many request messages the API has.
        pub const COUNT: usize = [$(stringify!($request)),+].len();

        /// Decodes `bytes` as the `which`th request message; whether prost took it.
        #[must_use]
        pub fn decode(which: usize, bytes: &[u8]) -> bool {
            let table: [fn(&[u8]) -> bool; COUNT] =
                [$(|b| <$request>::decode(b).is_ok()),+];
            table.get(which % COUNT).is_some_and(|f| f(bytes))
        }
    };
}

requests!(
    admin::GetServerRequest,
    admin::NegotiateProtocolRequest,
    entity::CreateEntityRequest,
    entity::GetEntityRequest,
    entity::ListEntitiesRequest,
    entity::UpdateEntityRequest,
    entity::DeleteEntityRequest,
    entity::BatchCreateEntitiesRequest,
    query::QuerySamplesRequest,
    query::BrowseSamplesRequest,
    query::FindEventsRequest,
    query::ComputeStatsRequest,
    query::ComparePeriodsRequest,
    replay::PauseReplaySessionRequest,
    replay::ResumeReplaySessionRequest,
    replay::SeekReplaySessionRequest,
    replay::StepReplaySessionRequest,
    replay::StopReplaySessionRequest,
    replay::DriveReplaySessionRequest,
    replay::CreateReplaySessionRequest,
    replay::GetReplaySessionRequest,
    replay::ListReplaySessionsRequest,
    replay::UpdateReplaySessionRequest,
    replay::DeleteReplaySessionRequest,
    signal::CreateSignalRequest,
    signal::GetSignalRequest,
    signal::ListSignalsRequest,
    signal::UpdateSignalRequest,
    signal::DeleteSignalRequest,
    signal::BatchCreateSignalsRequest,
    state::ComputeStateRequest,
    stream::VerifyStreamRequest,
    stream::ApplyStreamRetentionRequest,
    stream::CreateStreamRequest,
    stream::GetStreamRequest,
    stream::ListStreamsRequest,
    stream::UpdateStreamRequest,
    stream::DeleteStreamRequest,
    stream::CompactStreamRequest,
    stream::ExportStreamRequest,
    telemetry::SubscribeEventsRequest,
);
