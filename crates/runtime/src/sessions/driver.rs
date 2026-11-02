//! The task behind one replay session: it is the session's only owner.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use norn_core::Timestamp;
use norn_replay::{Batch, Control, Next, ReplayError, Session, SessionView};
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;

use super::Shared;
use super::delivery::{Subscriber, offer, wait_for_room};
use crate::clock::Clock;
use crate::subscription::{End, Subscription, leave};

pub(super) type Reply<T> = oneshot::Sender<Result<T, ReplayError>>;

/// What a caller asks of a session's driver.
pub(super) enum Request {
    Control {
        control: Control,
        generation: Option<u64>,
        reply: Reply<SessionView>,
    },
    Attach {
        reply: Reply<Subscription<Arc<Batch>>>,
    },
}

/// Owns a [`Session`] and its subscribers, and acts on what the session decides.
///
/// A task per session rather than a shared one, so a subscriber stalling one replay
/// stalls nothing else. Every wait also listens for controls, so `:stop` works past
/// a stalled subscriber. Time comes only from the injected clock.
pub(super) struct Driver {
    pub(super) session: Option<Session>,
    pub(super) requests: mpsc::Receiver<Request>,
    pub(super) shared: Arc<Shared>,
    pub(super) clock: Arc<dyn Clock>,
    pub(super) token: CancellationToken,
    pub(super) subscribers: Vec<Subscriber>,
    pub(super) outbox: VecDeque<(u64, Arc<Batch>)>,
    pub(super) numbered: u64,
    pub(super) stepping: Option<Reply<SessionView>>,
    pub(super) stalled: bool,
}

impl Driver {
    pub(super) fn session(&self) -> Option<&Session> {
        self.session.as_ref()
    }

    /// Makes the session's view visible to callers. Before any reply, so a caller
    /// reading the session after its control returns sees what the control did.
    pub(super) fn publish(&self) {
        if let Some(session) = self.session() {
            self.shared.publish(session.view(), self.clock.now());
        }
    }

    pub(super) fn end_all(&mut self, end: &End) {
        for s in self.subscribers.drain(..) {
            leave(&s.end, end.clone());
        }
        self.outbox.clear();
    }

    pub(super) fn queue(&mut self, batches: Vec<Batch>) {
        for batch in batches {
            self.numbered += 1;
            self.outbox.push_back((self.numbered, Arc::new(batch)));
        }
    }

    /// Runs until asked to end. Every session ends here.
    pub(super) async fn drive(mut self) {
        while self.turn().await.is_some() {
            self.publish();
        }
        self.publish();
        self.end_all(&End::ShuttingDown);
    }

    /// One step. `None` ends the task.
    async fn turn(&mut self) -> Option<()> {
        if let Some((number, batch)) = self.outbox.front().cloned() {
            return self.deliver(number, &batch).await;
        }
        let wall = self.clock.now();
        match self.session.as_mut()?.poll(wall) {
            Next::Emit(batch) => {
                self.queue(vec![batch]);
                Some(())
            }
            Next::Read => self.read(wall).await,
            Next::Wait(wait) => self.idle(Some(wait)).await,
            _ => {
                self.step_done();
                self.idle(None).await
            }
        }
    }

    fn step_done(&mut self) {
        self.publish();
        if let Some(reply) = self.stepping.take()
            && let Some(session) = self.session()
        {
            let _ = reply.send(Ok(session.view()));
        }
    }

    async fn deliver(&mut self, number: u64, batch: &Arc<Batch>) -> Option<()> {
        if offer(&mut self.subscribers, number, batch) {
            self.outbox.pop_front();
            if self.stalled {
                self.stalled = false;
                let wall = self.clock.now();
                self.session.as_mut()?.release(wall);
            }
            return Some(());
        }
        if !self.stalled {
            self.stalled = true;
            let wall = self.clock.now();
            self.session.as_mut()?.hold(wall);
            self.publish();
        }
        tokio::select! {
            biased;
            () = self.token.cancelled() => None,
            request = self.requests.recv() => self.handle(request?).await,
            () = wait_for_room(&mut self.subscribers, number, batch) => Some(()),
        }
    }

    async fn idle(&mut self, wait: Option<Duration>) -> Option<()> {
        let clock = Arc::clone(&self.clock);
        let sleep = async {
            match wait {
                Some(d) => clock.sleep(d).await,
                None => std::future::pending().await,
            }
        };
        tokio::select! {
            biased;
            () = self.token.cancelled() => None,
            request = self.requests.recv() => self.handle(request?).await,
            () = sleep => Some(()),
        }
    }

    async fn read(&mut self, wall: Timestamp) -> Option<()> {
        let mut session = self.session.take()?;
        let read = tokio::task::spawn_blocking(move || {
            let result = session.fill(wall);
            (session, result)
        });
        let (session, result) = read.await.ok()?;
        self.session = Some(session);
        if let Err(e) = result {
            self.failed(e);
        }
        Some(())
    }
}
