//! A `REPLAY` subscription: one session's events, as its clock reaches them.

use std::sync::Arc;

use norn_protocol::telemetry::{ClockEvent, ClockEventKind, SubscriptionMode, event};
use norn_runtime::replay::{Batch, ClockKind, Payload};
use norn_runtime::{End, Subscription};
use tokio::sync::{OwnedSemaphorePermit, mpsc};
use tonic::Status;

use super::{Item, Numbering, Subscriptions, send, shutting_down};
use crate::convert::sample_to_wire;
use crate::replay::wire::{id_of, status};

pub(super) struct Replayed {
    id: String,
    source: Subscription<Arc<Batch>>,
    _permit: OwnedSemaphorePermit,
}

pub(super) async fn start(
    from: &Subscriptions,
    name: &str,
    permit: OwnedSemaphorePermit,
) -> Result<Replayed, Status> {
    let id = id_of(name, "replay_session")?;
    let source = from.replays.subscribe(id).await;
    Ok(Replayed {
        id: id.to_owned(),
        source: source.map_err(|e| status(&e, id))?,
        _permit: permit,
    })
}

fn clock(kind: ClockKind) -> event::Payload {
    let kind = match kind {
        ClockKind::Running => ClockEventKind::Running,
        ClockKind::Paused => ClockEventKind::Paused,
        ClockKind::Seeked => ClockEventKind::Seeked,
        ClockKind::SpeedChanged => ClockEventKind::SpeedChanged,
        ClockKind::Ended => ClockEventKind::Ended,
        ClockKind::Stopped => ClockEventKind::Stopped,
        _ => ClockEventKind::Unspecified,
    };
    event::Payload::ClockEvent(ClockEvent { kind: kind as i32 })
}

fn payload(p: &Payload) -> Result<event::Payload, Status> {
    let cannot = |e| Status::internal(format!("cannot send a stored value: {e}"));
    Ok(match p {
        Payload::Sample { signal, sample } => {
            event::Payload::Sample(sample_to_wire(sample, signal.to_string()).map_err(cannot)?)
        }
        Payload::State { signal, sample } => {
            event::Payload::StateSample(sample_to_wire(sample, signal.to_string()).map_err(cannot)?)
        }
        Payload::Clock(kind) => clock(*kind),
        _ => return Err(Status::internal("an event of an unknown kind")),
    })
}

impl Replayed {
    pub(super) async fn run(mut self, out: mpsc::Sender<Item>) {
        let mut numbering = Numbering::new(SubscriptionMode::Replay);
        loop {
            let next = tokio::select! {
                () = out.closed() => return,
                next = self.source.next() => next,
            };
            let batch = match next {
                Ok(batch) => batch,
                Err(End::Stopped(at)) => {
                    let last = numbering.event(at, clock(ClockKind::Stopped));
                    send(&out, vec![last]).await;
                    return;
                }
                Err(End::Failed(e)) => {
                    let _ = out.send(Err(status(&e, &self.id))).await;
                    return;
                }
                Err(_) => {
                    let _ = out.send(Err(shutting_down())).await;
                    return;
                }
            };
            let mut events = Vec::with_capacity(batch.events.len());
            for e in &batch.events {
                match payload(&e.payload) {
                    Ok(p) => events.push(numbering.event(e.clock_time, p)),
                    Err(s) => {
                        let _ = out.send(Err(s)).await;
                        return;
                    }
                }
            }
            if !send(&out, events).await {
                return;
            }
        }
    }
}
