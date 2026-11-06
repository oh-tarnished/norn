//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A fake `TelemetryService` that answers each batch as a script says and
//! remembers what it stored, over a real socket through the real client. It
//! stores a sample once, as Norn does, by its signal, time and sequence.

use crate::replies::{Replies, reply, shutting_down};
use norn_protocol::telemetry::telemetry_service_server::TelemetryService;
use norn_protocol::telemetry::{
    AcknowledgementLevel as Ack, AppendSamplesRequest, AppendSamplesResponse,
    BackpressureOutcome as O, BackpressureReason as R, SubscribeEventsRequest,
    SubscribeEventsResponse,
};
use std::collections::{HashSet, VecDeque};
use std::sync::{Arc, Mutex};
use std::time::Instant;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tonic::{Request, Response, Status, Streaming, async_trait};

/// One answer: what to say of a batch, and any delay with it.
pub type Step = (O, R, Option<i64>);

/// What the fake has seen. Shared between it and the test.
#[derive(Default)]
pub struct Seen {
    /// Answers still to give; past the end, `ACCEPT`.
    pub script: VecDeque<Step>,
    /// The sequence of every sample admitted, resends included, in order.
    pub arrived: Vec<i64>,
    /// The sequence of every sample stored, once each, in order.
    pub stored: Vec<i64>,
    /// Their keys.
    pub keys: HashSet<String>,
    /// When each batch came.
    pub arrivals: Vec<Instant>,
    /// Drain after this many batches: the next is neither stored nor answered,
    /// and the call ends `UNAVAILABLE` / `SHUTTING_DOWN`. Once.
    pub drain_after: Option<usize>,
    /// Lost after this many batches: the next is stored and never answered, and
    /// the call dies, so the client cannot know. Once.
    pub die_after: Option<usize>,
    /// The signals a registry has made, when there is one, and how many samples
    /// arrived for a signal it had not: there should be none.
    pub made: Option<crate::registry::Made>,
    pub early: usize,
}

pub type Log = Arc<Mutex<Seen>>;

/// The fake `TelemetryService` over `Log`; `CancellationToken` ends its calls.
#[derive(Clone)]
pub struct Fake(pub Log, pub CancellationToken);

/// What a batch does to the fake.
enum Then {
    Answer(Step, i64),
    End(Status),
}

fn store(seen: &mut Seen, batch: &AppendSamplesRequest) -> i64 {
    let mut duplicates = 0;
    for sample in &batch.samples {
        let time = sample.observe_time.as_ref().map(|t| (t.seconds, t.nanos));
        let key = format!("{}|{time:?}|{}", sample.signal, sample.sequence);
        seen.arrived.push(sample.sequence);
        if seen.keys.insert(key) {
            seen.stored.push(sample.sequence);
        } else {
            duplicates += 1;
        }
    }
    duplicates
}

fn take(log: &Log, batch: &AppendSamplesRequest) -> Then {
    let mut guard = log.lock().unwrap();
    let seen = &mut *guard;
    let count = seen.arrivals.len();
    if seen.drain_after.is_some_and(|n| count >= n) {
        seen.drain_after = None;
        return Then::End(shutting_down());
    }
    seen.arrivals.push(Instant::now());
    if seen.die_after.is_some_and(|n| count >= n) {
        seen.die_after = None;
        store(seen, batch);
        return Then::End(Status::unavailable("died before it could answer"));
    }
    let step = seen.script.pop_front();
    let step = step.unwrap_or((O::Accept, R::CapacityAvailable, None));
    if let Some(made) = &seen.made {
        let made = made.lock().unwrap();
        let early = batch.samples.iter();
        seen.early += early.filter(|s| !made.contains(&s.signal)).count();
    }
    let duplicates = if step.0 == O::Reject {
        0
    } else {
        store(seen, batch)
    };
    Then::Answer(step, duplicates)
}

async fn answer(
    log: Log,
    gone: CancellationToken,
    mut batches: Streaming<AppendSamplesRequest>,
    out: mpsc::Sender<Result<AppendSamplesResponse, Status>>,
) {
    loop {
        let batch = tokio::select! {
            () = gone.cancelled() => {
                let _ = out.send(Err(Status::unavailable("the daemon went away"))).await;
                return;
            }
            got = batches.message() => match got {
                Ok(Some(batch)) => batch,
                _ => return,
            },
        };
        let id = batch.batch_id;
        match take(&log, &batch) {
            Then::End(status) => {
                let _ = out.send(Err(status)).await;
                return;
            }
            Then::Answer(step, _) if step.0 == O::Reject => {
                let _ = out.send(Ok(reply(id, step, Ack::Unspecified))).await;
            }
            Then::Answer(step, duplicate_count) => {
                let mut first = reply(id, step, Ack::Buffered);
                first.duplicate_count = duplicate_count;
                let _ = out.send(Ok(first)).await;
                let _ = out.send(Ok(reply(id, step, Ack::Persisted))).await;
            }
        }
    }
}

#[async_trait]
impl TelemetryService for Fake {
    type AppendSamplesStream = Replies<Result<AppendSamplesResponse, Status>>;
    type SubscribeEventsStream = Replies<Result<SubscribeEventsResponse, Status>>;

    async fn append_samples(
        &self,
        request: Request<Streaming<AppendSamplesRequest>>,
    ) -> Result<Response<Self::AppendSamplesStream>, Status> {
        let (tx, rx) = mpsc::channel(32);
        let (log, gone) = (self.0.clone(), self.1.clone());
        tokio::spawn(answer(log, gone, request.into_inner(), tx));
        Ok(Response::new(Replies(rx)))
    }

    async fn subscribe_events(
        &self,
        _: Request<SubscribeEventsRequest>,
    ) -> Result<Response<Self::SubscribeEventsStream>, Status> {
        Err(Status::unimplemented("subscribe_events"))
    }
}
