//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Getting a session its connection and its call, in bounded time.

use super::{End, connect, ended, lost, negotiate, plane};
use crate::provision;
use crate::shared::Shared;
use norn_ipc::{AppendReplies, AppendSender};
use std::time::Duration;
use tokio::time::{sleep, timeout};
use tonic::transport::Channel;

/// How long dialling, negotiating and opening the call may take: a daemon that
/// accepts and never answers is as away as one that refuses.
const OPEN_WITHIN: Duration = Duration::from_secs(30);

/// A session's connections and its call.
pub(super) struct Opened {
    pub(super) channels: Vec<Channel>,
    pub(super) sender: AppendSender,
    pub(super) replies: AppendReplies,
}

/// Dials, negotiates, makes the stream, opens the one `AppendSamples` call, and
/// dials more connections for creating resources: one the daemon will not give
/// is simply not used.
async fn open(shared: &Shared) -> Result<Opened, End> {
    let channel = connect(shared).await?;
    negotiate(shared, channel.clone()).await?;
    provision::stream(channel.clone(), &shared.stream).await?;
    let call = plane::open(shared, channel.clone())
        .await
        .map_err(|s| ended(&s))?;
    let (sender, replies) = call.into_parts();
    let mut channels = vec![channel];
    while channels.len() < shared.options.connections {
        match connect(shared).await {
            Ok(more) => channels.push(more),
            Err(_) => break,
        }
    }
    Ok(Opened {
        channels,
        sender,
        replies,
    })
}

/// Resolves [`Options::drain`](crate::Options::drain) after `close`: a session
/// still trying to open by then is given up.
async fn drain_over(shared: &Shared) {
    shared.closed().await;
    sleep(shared.options.drain).await;
}

/// [`open`], given up after [`OPEN_WITHIN`] or the drain.
pub(super) async fn open_within(shared: &Shared) -> Result<Opened, End> {
    let opened = tokio::select! {
        opened = timeout(OPEN_WITHIN, open(shared)) => opened,
        () = drain_over(shared) => return Err(lost("closed before Norn answered")),
    };
    opened.unwrap_or_else(|_| Err(lost("Norn did not answer in time")))
}
