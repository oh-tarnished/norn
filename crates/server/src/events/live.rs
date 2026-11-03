//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A `LIVE` subscription: committed samples of the selected signals.

use std::collections::HashMap;
use std::sync::Arc;

use norn_core::SignalId;
use norn_protocol::common::ErrorReason;
use norn_protocol::telemetry::{SubscriptionMode, event};
use norn_runtime::live::LiveBatch;
use norn_runtime::{Clock, End, Subscription};
use tokio::sync::{OwnedSemaphorePermit, mpsc};
use tonic::{Code, Status};

use super::{Item, Numbering, Subscriptions, full, send, shutting_down};
use crate::convert::sample_to_wire;
use crate::query::request::targets;
use crate::status::error_info;

pub(super) struct Live {
    names: HashMap<SignalId, Arc<str>>,
    source: Subscription<Arc<LiveBatch>>,
    clock: Arc<dyn Clock>,
    _permit: OwnedSemaphorePermit,
}

/// Resolves the selection now, so a signal created later does not join, and
/// attaches before returning: every sample committed from here on is delivered.
pub(super) async fn start(
    from: &Subscriptions,
    parent: &str,
    filter: &str,
    permit: OwnedSemaphorePermit,
) -> Result<Live, Status> {
    let registry = Arc::clone(&from.registry);
    let (parent, filter) = (parent.to_owned(), filter.to_owned());
    let selected = tokio::task::spawn_blocking(move || targets(&registry, &parent, &filter))
        .await
        .map_err(|_| Status::internal("resolving signals panicked"))??;
    let names = selected.into_iter().map(|t| (t.signal, t.name)).collect();
    let source = from.ingest.live().subscribe().ok_or_else(full)?;
    Ok(Live {
        names,
        source,
        clock: Arc::clone(&from.clock),
        _permit: permit,
    })
}

fn too_slow() -> Status {
    let text = "this subscriber fell behind its bounded queue; resubscribe, and query the gap";
    error_info(
        Code::ResourceExhausted,
        ErrorReason::SubscriberTooSlow,
        text,
        [],
    )
}

impl Live {
    pub(super) async fn run(mut self, out: mpsc::Sender<Item>) {
        let mut numbering = Numbering::new(SubscriptionMode::Live);
        loop {
            let next = tokio::select! {
                () = out.closed() => return,
                next = self.source.next() => next,
            };
            let batch = match next {
                Ok(batch) => batch,
                Err(End::TooSlow) => {
                    let _ = out.send(Err(too_slow())).await;
                    return;
                }
                Err(_) => {
                    let _ = out.send(Err(shutting_down())).await;
                    return;
                }
            };
            let now = self.clock.now();
            let mut events = Vec::new();
            for sample in &batch.samples {
                let Some(name) = self.names.get(&sample.signal) else {
                    continue;
                };
                let Ok(wire) = sample_to_wire(sample, name.to_string()) else {
                    let _ = out
                        .send(Err(Status::internal("cannot send a sample")))
                        .await;
                    return;
                };
                events.push(numbering.event(now, event::Payload::Sample(wire)));
            }
            if !send(&out, events).await {
                return;
            }
        }
    }
}
