//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One chunk of the work, taken, carried out, and either finished or put back.

use super::End;
use crate::health::Notice;
use crate::provision::{self, Kind, Resource};
use crate::shared::Shared;
use tonic::transport::Channel;

/// A chunk taken from the work, put back when dropped unless it was done: on a
/// daemon gone away, and on a session ending with it still on the wire, which
/// drops the future that holds this.
struct Held<'a> {
    shared: &'a Shared,
    kind: Kind,
    taken: Option<Vec<Resource>>,
}

impl Drop for Held<'_> {
    fn drop(&mut self) {
        if let Some(taken) = self.taken.take() {
            self.shared.state().work.put_back(self.kind, taken);
        }
    }
}

/// Carries out one chunk of the work, entities, then signals, then updates,
/// and lets the readings of the signals it made go.
pub(super) async fn provide(shared: &Shared, channels: &[Channel]) -> Result<(), End> {
    let (taken, max_bytes) = {
        let mut state = shared.state();
        let most = state
            .work
            .next()
            .map(|kind| provision::chunk_size(kind, channels.len()));
        let taken = most.and_then(|most| state.work.take(most));
        let limit = super::limits::message_limit(state.published_limit, &shared.options);
        (taken, super::limits::batch_bytes(limit))
    };
    let Some((kind, taken)) = taken else {
        return Ok(());
    };
    let mut held = Held {
        shared,
        kind,
        taken: Some(taken.clone()),
    };
    let done = provision::chunk(channels, kind, taken, max_bytes).await?;
    held.taken = None;
    let caught_up = {
        let mut state = shared.state();
        for resource in done.stale {
            state.work.update(resource);
        }
        state.made.extend(done.made.iter().cloned());
        state.flow.made(&done.made);
        let last = !done.made.is_empty() && state.work.creates() == 0;
        last.then(|| state.made.len())
    };
    if let Some(signals) = caught_up {
        shared.tell(&Notice::Provisioned { signals });
    }
    for why in done.refused {
        shared.tell(&Notice::ResourceRefused { why });
    }
    Ok(())
}
