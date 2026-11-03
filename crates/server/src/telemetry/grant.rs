//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Starting a call on whichever plane it gets, and telling the client which.

use std::sync::Arc;

use norn_protocol::telemetry::AppendSamplesRequest;
use norn_protocol::{
    DATA_PLANE_METADATA_KEY, DATA_PLANE_SHARED_MEMORY, SHARED_MEMORY_SERVICE_METADATA_KEY,
};
use tokio::sync::mpsc;
use tonic::metadata::MetadataValue;
use tonic::{Request, Response, Streaming};

use super::source::Source;
use super::{Responses, Telemetry, forward};
use crate::metered::Held;

impl Telemetry {
    /// The call's input: a ring when the client asked for one and the plane
    /// grants it, with the ring's service name to announce; gRPC otherwise.
    pub(super) async fn source(
        &self,
        request: Request<Streaming<AppendSamplesRequest>>,
    ) -> (Option<String>, Source) {
        let held = request.extensions().get::<Arc<Held>>().cloned();
        let asked = request
            .metadata()
            .get(DATA_PLANE_METADATA_KEY)
            .is_some_and(|v| v.as_bytes() == DATA_PLANE_SHARED_MEMORY.as_bytes());
        let attached = match &self.plane {
            Some(plane) if asked => plane.attach().await,
            _ => None,
        };
        let input = request.into_inner();
        match attached {
            Some((service, ring)) => (Some(service), Source::ring(ring, input, held)),
            None => (None, Source::Grpc(Box::new(input), held)),
        }
    }

    /// Starts the call's task on the tracked set. Whichever plane `input` is,
    /// everything after this point is the same code.
    pub(super) fn drive(&self, input: Source) -> Responses {
        let capacity = self.max_pending.saturating_mul(2).max(16);
        let (out, responses) = mpsc::channel(capacity);
        let (waits_tx, waits) = mpsc::channel(self.max_pending.max(1));
        let reader = self.clone().read(input, out.clone(), waits_tx);
        let last = out.clone();
        self.calls.spawn(async move {
            let (drained, ()) = tokio::join!(reader, forward(waits, out));
            // Answer every admitted batch before reporting why the call ends.
            if drained {
                let _ = last.send(Err(crate::events::shutting_down())).await;
            }
        });
        Responses(responses)
    }
}

/// Grants the ring named `service` in the response metadata. A name that is not
/// valid metadata is never granted: the ring then sits unused until the call
/// ends, and the client, having no name, stays on gRPC.
pub(super) fn announce(response: &mut Response<Responses>, service: Option<String>) {
    if let Some(value) = service.and_then(|s| MetadataValue::try_from(s).ok()) {
        response
            .metadata_mut()
            .insert(SHARED_MEMORY_SERVICE_METADATA_KEY, value);
    }
}
