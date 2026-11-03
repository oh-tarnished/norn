//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `TelemetryService.SubscribeEvents`: live and replayed telemetry, one event model.

mod live;
mod replayed;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_core::Timestamp;
use norn_metadata::Registry;
use norn_protocol::common::ErrorReason;
use norn_protocol::telemetry::{
    Event, SubscribeEventsRequest, SubscribeEventsResponse, SubscriptionMode, event,
};
use norn_runtime::Clock;
use norn_runtime::ingest::Ingest;
use norn_runtime::replay::MAX_BATCH_EVENTS;
use norn_runtime::sessions::Replays;
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc};
use tokio_util::task::TaskTracker;
use tonic::codegen::tokio_stream::Stream;
use tonic::{Code, Status};

use crate::convert::time_to_wire;
use crate::status::{bad_field, error_info};

/// Responses queued for a client. Small on purpose: a client that stops reading
/// fills it, and then the subscription's own bounded queue, and is handled there.
const QUEUED_RESPONSES: usize = 2;

type Item = Result<SubscribeEventsResponse, Status>;

/// One call's responses.
pub struct Events(mpsc::Receiver<Item>);

impl Stream for Events {
    type Item = Item;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Item>> {
        self.0.poll_recv(cx)
    }
}

/// Numbers a subscription's events: 1, then up by exactly one.
struct Numbering {
    mode: SubscriptionMode,
    next: i64,
}

impl Numbering {
    fn new(mode: SubscriptionMode) -> Self {
        Self { mode, next: 1 }
    }

    fn event(&mut self, clock: Timestamp, payload: event::Payload) -> Event {
        let event_sequence = self.next;
        self.next += 1;
        Event {
            mode: self.mode as i32,
            event_sequence,
            clock_time: Some(time_to_wire(clock)),
            payload: Some(payload),
        }
    }
}

/// Sends `events` in responses of at most [`MAX_BATCH_EVENTS`]. False once the
/// client has gone.
async fn send(out: &mpsc::Sender<Item>, events: Vec<Event>) -> bool {
    let mut events = events.into_iter().peekable();
    while events.peek().is_some() {
        let part: Vec<Event> = events.by_ref().take(MAX_BATCH_EVENTS).collect();
        let response = SubscribeEventsResponse { events: part };
        if out.send(Ok(response)).await.is_err() {
            return false;
        }
    }
    true
}

/// Where subscriptions are made.
///
/// Concurrency: `Send + Sync`, shared. Each subscription is one task, which ends
/// when the client goes, the source ends, or the daemon shuts down. Memory: at
/// most `max_subscriptions` at once, each holding two responses and its source's
/// bounded queue.
#[derive(Debug)]
pub struct Subscriptions {
    pub(crate) registry: Arc<Registry>,
    pub(crate) ingest: Arc<Ingest>,
    pub(crate) replays: Arc<Replays>,
    pub(crate) clock: Arc<dyn Clock>,
    permits: Arc<Semaphore>,
    calls: TaskTracker,
}

impl Subscriptions {
    /// At most `limit` subscriptions at once, of both modes together.
    #[must_use]
    pub fn new(
        registry: Arc<Registry>,
        ingest: Arc<Ingest>,
        replays: Arc<Replays>,
        clock: Arc<dyn Clock>,
        limit: usize,
    ) -> Self {
        let permits = Arc::new(Semaphore::new(limit));
        Self {
            registry,
            ingest,
            replays,
            clock,
            permits,
            calls: TaskTracker::new(),
        }
    }

    /// Runs each subscription's task on `calls`, which shutdown waits for.
    #[must_use]
    pub fn tracked(mut self, calls: TaskTracker) -> Self {
        self.calls = calls;
        self
    }

    fn permit(&self) -> Result<OwnedSemaphorePermit, Status> {
        Arc::clone(&self.permits)
            .try_acquire_owned()
            .map_err(|_| full())
    }

    /// Starts a subscription.
    ///
    /// # Errors
    /// `INVALID_ARGUMENT` for fields that do not fit the mode; `RESOURCE_EXHAUSTED`
    /// with `ERROR_REASON_CAPACITY_EXHAUSTED` past `max_subscriptions`; for a replay,
    /// whatever naming its session gives.
    pub async fn subscribe(&self, r: SubscribeEventsRequest) -> Result<Events, Status> {
        let (out, events) = mpsc::channel(QUEUED_RESPONSES);
        match SubscriptionMode::try_from(r.mode) {
            Ok(SubscriptionMode::Live) => {
                if !r.replay_session.is_empty() {
                    return Err(bad_field("replay_session", "must be empty for LIVE"));
                }
                let task = live::start(self, &r.parent, &r.filter, self.permit()?).await?;
                self.calls.spawn(task.run(out));
            }
            Ok(SubscriptionMode::Replay) => {
                if !r.parent.is_empty() || !r.filter.is_empty() {
                    return Err(bad_field(
                        "parent",
                        "parent and filter must be empty for REPLAY",
                    ));
                }
                let permit = self.permit()?;
                let task = replayed::start(self, &r.replay_session, permit).await?;
                self.calls.spawn(task.run(out));
            }
            _ => return Err(bad_field("mode", "is required")),
        }
        Ok(Events(events))
    }
}

fn full() -> Status {
    let limit = [("limit", "max_subscriptions".to_owned())];
    let text = "max_subscriptions subscriptions are open";
    error_info(
        Code::ResourceExhausted,
        ErrorReason::CapacityExhausted,
        text,
        limit,
    )
}

/// `UNAVAILABLE`, `ERROR_REASON_SHUTTING_DOWN`: what a call cut short by the
/// drain ends with.
pub(crate) fn shutting_down() -> Status {
    let text = "the daemon is shutting down";
    error_info(Code::Unavailable, ErrorReason::ShuttingDown, text, [])
}
