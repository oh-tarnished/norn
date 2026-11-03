//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A session's driver acting on what callers ask.

use std::sync::Arc;

use norn_replay::{Batch, Control, ReplayError, Session, SessionView};
use tokio::sync::mpsc;

use super::Attached;
use super::delivery::Subscriber;
use super::driver::{Driver, Reply, Request};
use crate::subscription::{End, EndSlot, Subscription, leave};

/// Batches queued per subscriber. Small: the clock waits for the slowest, so a
/// queue here is only smoothing, and every batch in one is memory held.
const SUBSCRIBER_QUEUE: usize = 4;

impl Driver {
    /// History could not be read. Subscribers hear why after what they already have,
    /// and whoever was stepping hears it too. The session stays, paused.
    pub(super) fn failed(&mut self, e: ReplayError) {
        self.publish();
        if let Some(reply) = self.stepping.take() {
            let _ = reply.send(Err(e.clone()));
        }
        for s in self.subscribers.drain(..) {
            leave(&s.end, End::Failed(e.clone()));
        }
    }

    pub(super) async fn handle(&mut self, request: Request) -> Option<()> {
        match request {
            Request::Attach { reply } => {
                let _ = reply.send(self.attach());
            }
            Request::Control {
                control,
                generation,
                reply,
            } => self.control(control, generation, reply).await?,
        }
        Some(())
    }

    fn attach(&mut self) -> Result<Subscription<Arc<Batch>>, ReplayError> {
        let (tx, rx) = mpsc::channel(SUBSCRIBER_QUEUE);
        let end = EndSlot::default();
        let next = self.numbered + 1;
        let keep = Arc::clone(&end);
        self.subscribers.push(Subscriber {
            tx,
            next,
            end: keep,
        });
        Ok(Subscription {
            rx,
            end,
            _guard: Some(Attached::new(Arc::clone(&self.shared))),
        })
    }

    async fn control(
        &mut self,
        mut control: Control,
        generation: Option<u64>,
        reply: Reply<SessionView>,
    ) -> Option<()> {
        let view = self.session()?.view();
        if generation.is_some_and(|g| g != view.generation) {
            let _ = reply.send(Err(ReplayError::EtagMismatch));
            return Some(());
        }
        if let Control::Seek { report_state, .. } = &mut control {
            *report_state = !self.subscribers.is_empty();
        }
        let wall = self.clock.now();
        let mut session = self.session.take()?;
        let applied = tokio::task::spawn_blocking(move || {
            let result = session.control(control, wall);
            (session, result)
        });
        let (session, result) = applied.await.ok()?;
        self.session = Some(session);
        match result {
            Ok(batches) => {
                self.queue(batches);
                self.after(control, reply);
            }
            Err(e) => {
                let _ = reply.send(Err(e));
            }
        }
        Some(())
    }

    fn after(&mut self, control: Control, reply: Reply<SessionView>) {
        self.publish();
        let view = self.session().map(Session::view);
        match (control, view) {
            (Control::StepEvents(_) | Control::StepTime(_), _) => self.stepping = Some(reply),
            (Control::Stop, Some(view)) => {
                if let Some(pending) = self.stepping.take() {
                    let _ = pending.send(Ok(view));
                }
                let at = view.clock.now(self.clock.now());
                self.end_all(&End::Stopped(at));
                let _ = reply.send(Ok(view));
            }
            (_, Some(view)) => {
                let _ = reply.send(Ok(view));
            }
            (_, None) => {}
        }
    }
}
