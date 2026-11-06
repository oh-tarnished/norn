//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One connection to the daemon, from dialling to whatever ends it. Everything
//! here runs on the task awaiting [`Sink::run`](crate::Sink::run), so whatever a
//! session started ends with it.

use super::open::{Opened, open_within};
use super::provide::provide;
use super::{End, ended, lost, plane};
use crate::counts::Backoff;
use crate::flow::Outcome;
use crate::shared::Shared;
use norn_ipc::{AppendReplies, AppendSender};
use norn_protocol::telemetry::AppendSamplesRequest;
use std::future::Future;
use std::pin::Pin;
use std::time::Duration;
use tokio::sync::mpsc;
use tokio::time::{Instant, sleep_until, timeout};

/// How long a call whose sending half failed gets to say why.
const WHY_WITHIN: Duration = Duration::from_secs(1);

/// A boxed future the session polls beside the sends.
type Beside<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Carries batches from the session's queue onto the call, waiting while the
/// plane is full. Separate from the session's loop because a send interrupted
/// by a reply would lose its batch. Resolves only when a send fails.
async fn forward(
    sender: AppendSender,
    mut batches: mpsc::Receiver<AppendSamplesRequest>,
) -> String {
    while let Some(batch) = batches.recv().await {
        if let Err(e) = sender.send(batch).await {
            return e.to_string();
        }
    }
    "the session's queue closed".to_owned()
}

/// Takes one reply to the flow; `Some` if it ends the session.
fn taken(
    shared: &Shared,
    reply: &norn_protocol::telemetry::AppendSamplesResponse,
    origin: Instant,
) -> Option<End> {
    let told = shared.state().flow.answer(reply, origin.elapsed());
    match told {
        Outcome::Fine => None,
        Outcome::Tell(notice) => {
            shared.tell(&notice);
            None
        }
        Outcome::Reconnect(why) => Some(lost(why)),
    }
}

/// How a call whose sending half failed with `why` ended: its own status says,
/// so a drain is not mistaken for a loss. Answers still coming are taken, so
/// nothing already acknowledged is resent.
async fn why_ended(
    shared: &Shared,
    replies: &mut AppendReplies,
    origin: Instant,
    why: String,
) -> End {
    let last = timeout(WHY_WITHIN, async {
        loop {
            match replies.message().await {
                Ok(Some(reply)) => {
                    if let Some(end) = taken(shared, &reply, origin) {
                        return end;
                    }
                }
                Ok(None) => return lost(&why),
                Err(status) => return ended(&status),
            }
        }
    });
    last.await.unwrap_or_else(|_| lost(&why))
}

/// The future in `beside` finishing; never, when there is none.
async fn poll<T>(beside: &mut Option<Beside<'_, T>>) -> T {
    match beside {
        Some(future) => future.as_mut().await,
        None => std::future::pending().await,
    }
}

/// Serves one connection until it ends.
pub(super) async fn session(shared: &Shared, origin: Instant, backoff: &mut Backoff) -> End {
    let Opened {
        channels,
        sender,
        mut replies,
    } = match open_within(shared).await {
        Ok(opened) => opened,
        Err(end) => return end,
    };
    shared.set_reach(true, "Norn reachable".to_owned());
    backoff.reset();
    let (tx, rx) = mpsc::channel(plane::QUEUED);
    let mut forwarding: Option<Beside<'_, String>> = Some(Box::pin(forward(sender, rx)));
    let mut stopping: Option<Instant> = None;
    // The chunk being created, polled beside the sends rather than awaited
    // before them: a batch create's round trip must not pace the readings of
    // signals already made. One at a time, so creates stay in order.
    let mut creating: Option<Beside<'_, Result<(), End>>> = None;
    loop {
        if creating.is_none() && !shared.state().work.is_empty() {
            creating = Some(Box::pin(provide(shared, &channels)));
        }
        let now: Duration = origin.elapsed();
        let (ready, wake, idle) = {
            let state = shared.state();
            let idle = creating.is_none() && state.work.is_empty() && state.flow.held() == 0;
            // Readings go whatever is still being created: those whose signal
            // is not yet made wait in the flow, not here.
            (state.flow.ready(now), state.flow.wake(now), idle)
        };
        if stopping.is_some_and(|by| idle || Instant::now() >= by) {
            return End::Finished;
        }
        let at = [wake.map(|d| origin + d), stopping]
            .into_iter()
            .flatten()
            .min();
        tokio::select! {
            // First: `biased` stops at the first branch ready, and replies and
            // permits are ready almost always at scale, so these polled after
            // them would starve. Each is ready only when it ends.
            biased;
            made = poll(&mut creating), if creating.is_some() => {
                creating = None;
                if let Err(end) = made {
                    return end;
                }
            }
            why = poll(&mut forwarding), if forwarding.is_some() => {
                return why_ended(shared, &mut replies, origin, why).await;
            }
            reply = replies.message() => match reply {
                Ok(Some(reply)) => {
                    if let Some(end) = taken(shared, &reply, origin) {
                        return end;
                    }
                }
                Ok(None) => return lost("the call closed"),
                Err(status) => return ended(&status),
            },
            permit = tx.reserve(), if ready => match permit {
                Ok(permit) => {
                    if let Some(batch) = shared.state().flow.take(origin.elapsed()) {
                        permit.send(batch);
                    }
                }
                Err(e) => return lost(e),
            },
            () = shared.closed(), if stopping.is_none() => {
                stopping = Some(Instant::now() + shared.options.drain);
            }
            () = shared.wake.notified() => {}
            () = sleep_until(at.unwrap_or_else(Instant::now)), if at.is_some() => {}
        }
    }
}
