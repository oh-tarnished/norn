//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The load `nornctl benchmark` applies: paced `AppendSamples` batches sized
//! from the daemon's published limits.
//!
//! An acknowledgement's latency runs from the batch's send to the reply at the
//! requested level. A daemon that cannot keep up slows the sender, since at
//! most `max_pending_batches` are outstanding, so read the achieved rate beside
//! the latency: it does not count time a batch waited to be sent.

use std::collections::HashMap;
use std::io::Write;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use norn_ipc::{AppendCall, AppendOptions};
use norn_protocol::admin::Limits;
use norn_protocol::telemetry::{AppendSamplesRequest, BackpressureOutcome};
use tonic::transport::Channel;

use super::batch::{now_nanos, sample};
use super::plan::Plan;
use super::query::percentile;
use crate::CliError;
use crate::resources::call;

const APPEND: &str = "TelemetryService.AppendSamples";
/// What the append phase measured.
#[derive(Debug, Default)]
pub(super) struct Appended {
    /// Nanoseconds since the epoch of the first sample sent.
    pub first: i128,
    samples: u64,
    batches: u64,
    rejected: u64,
    duplicates: i64,
    elapsed: Duration,
    latencies: Vec<Duration>,
}

impl Appended {
    pub(super) fn print(&self, out: &mut impl Write) -> std::io::Result<()> {
        let rate = self.samples as f64 / self.elapsed.as_secs_f64().max(f64::MIN_POSITIVE);
        writeln!(out, "append.samples: {}", self.samples)?;
        writeln!(out, "append.batches: {}", self.batches)?;
        writeln!(out, "append.rejected_batches: {}", self.rejected)?;
        writeln!(out, "append.duplicates: {}", self.duplicates)?;
        writeln!(out, "append.samples_per_second: {rate:.0}")?;
        writeln!(
            out,
            "append.ack_p50_ms: {:.3}",
            percentile(&self.latencies, 50)
        )?;
        writeln!(
            out,
            "append.ack_p99_ms: {:.3}",
            percentile(&self.latencies, 99)
        )
    }
}

type Pending = Arc<Mutex<HashMap<i64, (Instant, u64)>>>;

fn lock(pending: &Pending) -> std::sync::MutexGuard<'_, HashMap<i64, (Instant, u64)>> {
    pending.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Sends batches for `plan.duration`, then waits for every acknowledgement.
pub(super) async fn append(
    channel: &Channel,
    plan: &Plan,
    limits: &Limits,
    batch: usize,
) -> Result<Appended, CliError> {
    let opened = AppendCall::open(channel.clone(), &AppendOptions::default()).await;
    let (sender, mut replies) = opened.map_err(call(APPEND))?.into_parts();
    let pending: Pending = Arc::default();
    let requested = plan.ack as i32;
    let tally = Arc::clone(&pending);
    let reader = tokio::spawn(async move {
        let mut seen = Appended::default();
        let mut refusal = None;
        loop {
            let reply = match replies.message().await {
                Ok(Some(reply)) => reply,
                Ok(None) => return Ok((seen, refusal)),
                Err(status) => return Err(status),
            };
            let admission = reply.admission.clone().unwrap_or_default();
            let rejected = admission.outcome == BackpressureOutcome::Reject as i32;
            if rejected || reply.error.is_some() {
                lock(&tally).remove(&reply.batch_id);
                seen.rejected += 1;
                let text = reply.error.map_or(admission.message, |e| e.message);
                refusal.get_or_insert(text);
            } else if reply.ack_level >= requested
                && let Some((sent, samples)) = lock(&tally).remove(&reply.batch_id)
            {
                seen.latencies.push(sent.elapsed());
                seen.samples += samples;
                seen.batches += 1;
                seen.duplicates += reply.duplicate_count;
            }
        }
    });
    let most_pending = usize::try_from(limits.max_pending_batches)
        .unwrap_or(1)
        .max(1);
    let interval = plan
        .rate
        .map(|r| Duration::from_secs_f64(batch as f64 / r as f64));
    let started = Instant::now();
    let first = now_nanos();
    let (mut next, mut sequence, mut batch_id) = (tokio::time::Instant::now(), 0u64, 0i64);
    while started.elapsed() < plan.duration {
        if lock(&pending).len() >= most_pending {
            tokio::time::sleep(Duration::from_millis(1)).await;
            continue;
        }
        if let Some(interval) = interval {
            tokio::time::sleep_until(next).await;
            next += interval;
        }
        let base = now_nanos();
        let samples = (0..batch as u64)
            .map(|offset| {
                sequence += 1;
                sample(plan, sequence, base + i128::from(offset))
            })
            .collect();
        batch_id += 1;
        lock(&pending).insert(batch_id, (Instant::now(), batch as u64));
        let request = AppendSamplesRequest {
            stream: format!("streams/{}", plan.stream),
            batch_id,
            samples,
            ack_level: requested,
        };
        if sender.send(request).await.is_err() {
            break;
        }
        tokio::task::yield_now().await;
    }
    drop(sender);
    let read = reader
        .await
        .map_err(|e| CliError::Refused(format!("the reader failed: {e}")))?;
    let (mut seen, refusal) = read.map_err(call(APPEND))?;
    seen.elapsed = started.elapsed();
    seen.first = first;
    seen.latencies.sort_unstable();
    match refusal {
        Some(text) if seen.batches == 0 => Err(CliError::Refused(text)),
        _ => Ok(seen),
    }
}
