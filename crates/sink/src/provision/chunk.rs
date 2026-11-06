//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One chunk of the work, carried out: creates in batches, and updates a
//! connection's limit at a time on each connection.

use super::upsert::{self, upsert};
use super::{Failure, Kind, PER_CONNECTION, Resource, batch, work};
use crate::join::join_all;
use tonic::transport::Channel;

#[cfg(test)]
#[path = "chunk_tests.rs"]
mod tests;

/// What one chunk did. Plain data.
#[derive(Debug, Default)]
pub(crate) struct Done {
    /// Refusals, one per resource given up.
    pub refused: Vec<String>,
    /// Signals now made, or given up: their readings may go.
    pub made: Vec<String>,
    /// Resources that exist and have changed, to bring up to date later.
    pub stale: Vec<Resource>,
}

/// The most of each kind one chunk takes: a thousand creates, cut into batches
/// under the message limit and sent one on each connection at once, and as
/// many updates as the connections carry.
///
/// Measured against Norn 1.0.0-rc.1 by the first adapter built on this code:
/// 68,321 signals made in 2 s once batches fit the limit, where every batch
/// past it fell back to one create at a time, about 1,100 a second.
#[must_use]
pub(crate) fn chunk_size(kind: Kind, channels: usize) -> usize {
    match kind {
        Kind::Entities | Kind::Signals => batch::BATCH,
        Kind::Updates => channels.max(1) * PER_CONNECTION,
    }
}

/// Carries out `taken`, all of `kind`, each batch at most `max_bytes`. Creates
/// go in batches, each on a connection of its own should a chunk hold several,
/// and what a batch could not take one at a time; what exists but has changed
/// comes back as [`Done::stale`], never updated here, so no reading waits on an
/// update. `Err` only when the daemon went away: the caller puts `taken` back,
/// and, being idempotent, it simply resumes.
pub(crate) async fn chunk(
    channels: &[Channel],
    kind: Kind,
    taken: Vec<Resource>,
    max_bytes: usize,
) -> Result<Done, Failure> {
    if channels.is_empty() {
        return Err(Failure::Lost("no connection".to_owned()));
    }
    if kind == Kind::Updates {
        let refused = each(channels, taken, true).await?;
        return Ok(Done {
            refused,
            ..Done::default()
        });
    }
    let made = if kind == Kind::Signals {
        taken.iter().map(|r| work::name(r).to_owned()).collect()
    } else {
        Vec::new()
    };
    // A batch on each connection at once: no signal of one batch needs
    // another, and every entity of the chunk's signals was made before it.
    let calls = channels
        .iter()
        .cycle()
        .zip(cut(taken, max_bytes))
        .map(|(channel, one)| async move { batch::create(channel, one).await });
    let mut done = Done {
        made,
        ..Done::default()
    };
    let mut each_later = Vec::new();
    let mut lost = None;
    for result in join_all(calls.collect()).await {
        match result {
            Ok(out) => {
                done.refused.extend(out.refused);
                done.stale.extend(out.stale);
                each_later.extend(out.each);
            }
            Err(failure) => lost = Some(failure),
        }
    }
    // A batch lost puts the whole chunk back: creating is idempotent.
    if let Some(failure) = lost {
        return Err(failure);
    }
    done.refused
        .extend(each(channels, each_later, false).await?);
    Ok(done)
}

/// Creates (or, with `update`, only updates) `items` one at a time, a
/// connection's limit of calls on each of `channels` at once, and returns what
/// was refused.
async fn each(
    channels: &[Channel],
    items: Vec<Resource>,
    update: bool,
) -> Result<Vec<String>, Failure> {
    let mut refused = Vec::new();
    let mut items = items.into_iter().peekable();
    while items.peek().is_some() {
        let size = channels.len() * PER_CONNECTION;
        let calls =
            channels
                .iter()
                .cycle()
                .zip(items.by_ref().take(size))
                .map(|(channel, resource)| {
                    let channel = channel.clone();
                    async move {
                        if update {
                            upsert::update(channel, resource).await
                        } else {
                            upsert(channel, resource).await
                        }
                    }
                });
        let mut lost = None;
        for result in join_all(calls.collect()).await {
            match result {
                Ok(()) => {}
                Err(Failure::Refused(why)) => refused.push(why),
                Err(Failure::Lost(why)) => lost = Some(why),
            }
        }
        if let Some(why) = lost {
            return Err(Failure::Lost(why));
        }
    }
    Ok(refused)
}

/// Cuts `taken` into batches of at most [`batch::BATCH`] resources and
/// `max_bytes` encoded, in order. A resource larger alone goes alone.
fn cut(taken: Vec<Resource>, max_bytes: usize) -> Vec<Vec<Resource>> {
    let mut batches: Vec<Vec<Resource>> = Vec::new();
    let mut size = 0;
    for resource in taken {
        let bytes = encoded(&resource);
        let full = batches
            .last()
            .is_none_or(|b| b.len() >= batch::BATCH || (size + bytes > max_bytes && !b.is_empty()));
        if full {
            batches.push(Vec::new());
            size = 0;
        }
        size += bytes;
        if let Some(last) = batches.last_mut() {
            last.push(resource);
        }
    }
    batches
}

/// What `resource` adds to a batch request, near enough: its message and its
/// framing.
fn encoded(resource: &Resource) -> usize {
    use prost::Message as _;
    let body = match resource {
        Resource::Entity(e) => e.encoded_len(),
        Resource::Signal { parent, id, signal } => signal.encoded_len() + parent.len() + id.len(),
    };
    body + 16
}
