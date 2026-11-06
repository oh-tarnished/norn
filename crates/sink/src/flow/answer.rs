//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What each of Norn's answers means: SPEC §18's four outcomes, as protocol.md
//! states them, and the second response of SPEC §19.

use super::Flow;
use crate::health::Notice;
use norn_protocol::telemetry::{
    AppendSamplesResponse, BackpressureOutcome, BackpressureReason, SampleError,
};
use std::collections::BTreeSet;
use std::time::Duration;

/// gRPC's `NOT_FOUND`, which is what a batch for a missing stream carries.
const NOT_FOUND: i32 = 5;

/// What the caller must do about an answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// Nothing: this is the flow working.
    Fine,
    /// Nothing, but a person should hear this.
    Tell(Notice),
    /// The stream is gone from under the call: open again from the start.
    Reconnect(String),
}

fn delay(reply: &AppendSamplesResponse) -> Option<Duration> {
    let wait = reply.admission.as_ref()?.retry_delay.as_ref()?;
    let seconds = u64::try_from(wait.seconds).ok()?;
    let nanos = u32::try_from(wait.nanos).ok()?;
    Some(Duration::new(seconds, nanos))
}

impl Flow {
    /// Takes `reply` as of `now`. ACCEPT, BUFFER and THROTTLE all admit the
    /// batch, so it is never resent for those; THROTTLE alone also stops the
    /// next one until `retry_delay` has passed. An admitted batch stays held
    /// until it has reached the level asked for.
    pub(crate) fn answer(&mut self, reply: &AppendSamplesResponse, now: Duration) -> Outcome {
        let id = reply.batch_id;
        let wanted = self.ack as i32;
        let Some(batch) = self.inflight.get_mut(&id) else {
            return Outcome::Fine;
        };
        if !batch.admitted {
            let told = reply.admission.as_ref().map_or(0, |a| a.outcome);
            match BackpressureOutcome::try_from(told) {
                Ok(BackpressureOutcome::Accept | BackpressureOutcome::Buffer) => {}
                Ok(BackpressureOutcome::Throttle) => {
                    self.counts.throttled += 1;
                    let wait = delay(reply).unwrap_or_else(|| self.backoff.next_wait());
                    self.not_before = now + wait;
                }
                _ => return self.refused(reply, now),
            }
            self.counts.duplicates += reply.duplicate_count.unsigned_abs();
            self.backoff.reset();
            batch.admitted = true;
        }
        if let Some(error) = &reply.error {
            // Admitted, and it can no longer reach the level: send it again.
            let message = error.message.clone();
            self.hold(id, now, None);
            return Outcome::Tell(Notice::StoppedShort { batch: id, message });
        }
        if reply.ack_level >= wanted
            && let Some(done) = self.inflight.remove(&id)
        {
            self.counts.acknowledged += done.samples.len() as u64;
            for sample in &done.samples {
                self.ledger.remove(sample);
            }
        }
        Outcome::Fine
    }

    /// A batch refused whole: nothing of it is stored, so all of it stays,
    /// less any Norn says can never be stored. Which reasons a retry can cure
    /// is protocol.md's table; the rest are held too, behind the backoff,
    /// because they need a person, and the bounded buffer says how long that
    /// person has.
    fn refused(&mut self, reply: &AppendSamplesResponse, now: Duration) -> Outcome {
        use BackpressureReason as R;
        let id = reply.batch_id;
        self.counts.rejected += 1;
        let admission = reply.admission.as_ref();
        let reason = admission.map_or(0, |a| a.reason);
        let message = admission.map_or("", |a| a.message.as_str()).to_owned();
        if reply.error.as_ref().is_some_and(|e| e.code == NOT_FOUND) {
            self.hold(id, now, Some(Duration::ZERO));
            return Outcome::Reconnect(format!("batch {id} refused: {message}"));
        }
        match R::try_from(reason) {
            Ok(R::InvalidSamples) => self.strip(id, &reply.sample_errors),
            Ok(R::BatchTooLarge) => self.halve(id),
            Ok(
                R::BufferFull
                | R::MemoryExhausted
                | R::Throttled
                | R::ShuttingDown
                | R::ClientRateExceeded,
            ) => {
                self.hold(id, now, delay(reply));
                Outcome::Fine
            }
            _ => {
                self.hold(id, now, None);
                Outcome::Tell(Notice::Refused { batch: id, message })
            }
        }
    }

    /// Puts a batch back, nothing to go before `wait`, or the backoff.
    fn hold(&mut self, id: i64, now: Duration, wait: Option<Duration>) {
        if let Some(batch) = self.inflight.remove(&id) {
            self.requeue(batch.samples);
        }
        let wait = wait.unwrap_or_else(|| self.backoff.next_wait());
        self.not_before = self.not_before.max(now + wait);
    }

    /// Takes out every sample Norn listed, which only a change to the sample
    /// could cure, and sends the rest of the batch on. Nothing at all listed is
    /// a daemon this client cannot follow, so all of it is given up rather than
    /// sent again for ever.
    fn strip(&mut self, id: i64, errors: &[SampleError]) -> Outcome {
        let Some(batch) = self.inflight.remove(&id) else {
            return Outcome::Fine;
        };
        let bad: BTreeSet<_> = errors.iter().map(|e| e.index).collect();
        let total = batch.samples.len();
        let mut first = None;
        let mut kept = Vec::with_capacity(total);
        for (index, sample) in batch.samples.into_iter().enumerate() {
            let listed = i32::try_from(index).is_ok_and(|i| bad.contains(&i));
            if listed || bad.is_empty() {
                self.ledger.remove(&sample);
                first.get_or_insert(sample.signal);
            } else {
                kept.push(sample);
            }
        }
        let gone = (total - kept.len()) as u64;
        self.counts.invalid += gone;
        self.requeue(kept);
        let why = errors.first().and_then(|e| e.status.as_ref());
        Outcome::Tell(Notice::Invalid {
            readings: gone,
            first: first.unwrap_or_default(),
            why: why.map_or_else(String::new, |s| s.message.clone()),
        })
    }

    /// A batch over a limit the daemon did not publish: smaller from now on.
    /// One sample too large alone can never go, so it is given up.
    fn halve(&mut self, id: i64) -> Outcome {
        let Some(batch) = self.inflight.remove(&id) else {
            return Outcome::Fine;
        };
        let n = batch.samples.len();
        self.max_samples = self.max_samples.min(n / 2).max(1);
        if n == 1 {
            self.counts.invalid += 1;
            let mut first = String::new();
            for sample in batch.samples {
                self.ledger.remove(&sample);
                first = sample.signal;
            }
            let why = "too large alone for any batch".to_owned();
            return Outcome::Tell(Notice::Invalid {
                readings: 1,
                first,
                why,
            });
        }
        self.requeue(batch.samples);
        Outcome::Tell(Notice::TooLarge { readings: n })
    }
}
