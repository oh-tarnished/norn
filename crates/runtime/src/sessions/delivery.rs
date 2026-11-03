//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Handing batches to a session's subscribers.

use std::sync::Arc;

use norn_replay::Batch;
use tokio::sync::mpsc;

use crate::subscription::EndSlot;

/// One subscriber, as the driver holds it.
pub(super) struct Subscriber {
    pub(super) tx: mpsc::Sender<Arc<Batch>>,
    /// The number of the next batch it needs: delivery can be interrupted by a
    /// control and taken up again, and nobody gets a batch twice.
    pub(super) next: u64,
    pub(super) end: EndSlot,
}

/// Sends batch `number` to whoever still needs it, without waiting. True when
/// nobody does.
pub(super) fn offer(subscribers: &mut Vec<Subscriber>, number: u64, batch: &Arc<Batch>) -> bool {
    subscribers.retain(|s| !s.tx.is_closed());
    let mut all = true;
    for s in subscribers.iter_mut().filter(|s| s.next <= number) {
        match s.tx.try_send(Arc::clone(batch)) {
            Ok(()) => s.next = number + 1,
            Err(mpsc::error::TrySendError::Closed(_)) => {}
            Err(mpsc::error::TrySendError::Full(_)) => all = false,
        }
    }
    all
}

/// Waits for the first subscriber that needs `number` to have room, and gives it
/// the batch. Cancel-safe: dropped, nobody received it.
pub(super) async fn wait_for_room(subscribers: &mut [Subscriber], number: u64, batch: &Arc<Batch>) {
    let Some(s) = subscribers.iter_mut().find(|s| s.next <= number) else {
        return;
    };
    if let Ok(permit) = s.tx.reserve().await {
        permit.send(Arc::clone(batch));
        s.next = number + 1;
    }
}
