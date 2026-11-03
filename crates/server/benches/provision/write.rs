//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Creating the site: one resource per call, or many per batch call. Each
//! connection runs one call at a time, taking the next piece of work from a
//! shared counter, so `connections` is also the number of calls in flight.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use tokio::task::JoinSet;
use tonic::transport::Channel;

use crate::site::{Shape, entity, signal};

/// How resources are sent.
#[derive(Debug, Clone, Copy)]
pub enum Mode {
    /// `CreateEntity` and `CreateSignal`, one resource per call.
    OnePerCall,
    /// `BatchCreateEntities` and `BatchCreateSignals`, this many per call.
    Batched(usize),
}

impl Mode {
    pub fn label(self) -> String {
        match self {
            Self::OnePerCall => "Create, one per call".to_owned(),
            Self::Batched(n) => format!("BatchCreate, {n} per call"),
        }
    }

    fn per_call(self) -> usize {
        match self {
            Self::OnePerCall => 1,
            Self::Batched(n) => n,
        }
    }
}

/// Creates every entity, then every signal; returns how long each took.
pub async fn provision(channels: &[Channel], shape: Shape, mode: Mode) -> (Duration, Duration) {
    let start = Instant::now();
    run(channels, shape.devices, mode, move |c, range| {
        Box::pin(entities(c, range, mode))
    })
    .await;
    let entities = start.elapsed();
    let start = Instant::now();
    run(channels, shape.signals(), mode, move |c, range| {
        Box::pin(signals(c, range, shape, mode))
    })
    .await;
    (entities, start.elapsed())
}

type Work = std::pin::Pin<Box<dyn Future<Output = ()> + Send>>;

/// Hands out `[from, to)` ranges of `total` to one worker per channel.
async fn run(
    channels: &[Channel],
    total: usize,
    mode: Mode,
    work: impl Fn(Channel, std::ops::Range<usize>) -> Work + Send + Sync + 'static,
) {
    let next = Arc::new(AtomicUsize::new(0));
    let work = Arc::new(work);
    let step = mode.per_call();
    let mut tasks = JoinSet::new();
    for channel in channels {
        let (channel, next, work) = (channel.clone(), Arc::clone(&next), Arc::clone(&work));
        tasks.spawn(async move {
            loop {
                let from = next.fetch_add(step, Ordering::Relaxed);
                if from >= total {
                    break;
                }
                work(channel.clone(), from..(from + step).min(total)).await;
            }
        });
    }
    while let Some(done) = tasks.join_next().await {
        done.unwrap();
    }
}

async fn entities(channel: Channel, range: std::ops::Range<usize>, mode: Mode) {
    let mut client = EntityServiceClient::new(channel);
    match mode {
        Mode::OnePerCall => {
            for d in range {
                client.create_entity(entity(d)).await.unwrap();
            }
        }
        Mode::Batched(_) => crate::write_batch::entities(&mut client, range).await,
    }
}

async fn signals(channel: Channel, range: std::ops::Range<usize>, shape: Shape, mode: Mode) {
    let mut client = SignalServiceClient::new(channel);
    let point = |n: usize| (n / shape.points, n % shape.points);
    match mode {
        Mode::OnePerCall => {
            for n in range {
                let (d, p) = point(n);
                client.create_signal(signal(d, p)).await.unwrap();
            }
        }
        Mode::Batched(_) => crate::write_batch::signals(&mut client, range, point).await,
    }
}
