//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a source into the daemon.

use norn_protocol::admin::Capability;
use norn_protocol::common::Sample;
use norn_protocol::telemetry::AcknowledgementLevel;
use prost::Message;
use tonic::transport::Channel;

use crate::wire::appender::Appender;
use crate::wire::negotiate;
use crate::wire::present::{newest_times, without_present};
use crate::wire::registry::Registry;
use crate::{AdapterError, SampleSource, SourceBatch};

/// How an import runs. `Default` waits for `DURABLE`, as `nornctl append` does,
/// and uses the daemon's published batch limits.
#[derive(Debug, Clone)]
pub struct ImportOptions {
    level: AcknowledgementLevel,
    batch_samples: Option<usize>,
    max_retries: u32,
}

impl Default for ImportOptions {
    fn default() -> Self {
        Self {
            level: AcknowledgementLevel::Durable,
            batch_samples: None,
            max_retries: 1_000,
        }
    }
}

impl ImportOptions {
    /// The level each batch must reach before the next is sent.
    #[must_use]
    pub fn ack(mut self, level: AcknowledgementLevel) -> Self {
        self.level = level;
        self
    }

    /// At most this many samples a batch; never above the daemon's limit.
    #[must_use]
    pub fn batch_samples(mut self, samples: usize) -> Self {
        self.batch_samples = Some(samples);
        self
    }

    /// How many transient refusals of one batch are waited out before giving up.
    #[must_use]
    pub fn max_retries(mut self, retries: u32) -> Self {
        self.max_retries = retries;
        self
    }
}

/// What an import did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ImportReport {
    /// Samples read from the file.
    pub samples_read: u64,
    /// Samples the daemon took in, including any it counted as duplicates.
    pub samples_sent: u64,
    /// Samples read that the daemon already held, and so were never sent.
    pub samples_present: u64,
    /// Samples the daemon counted as duplicates of ones it held.
    pub samples_duplicate: u64,
    /// Streams, entities and signals created. Existing ones are left alone.
    pub resources_created: u64,
    /// Batches acknowledged.
    pub batches: u64,
    /// `THROTTLE` responses waited out.
    pub throttled: u64,
    /// Refusals waited out and retried.
    pub retried: u64,
}

/// Splits `samples` so each part fits the limits. `max_bytes` allows for framing.
fn split(
    samples: Vec<Sample>,
    max_samples: usize,
    max_bytes: usize,
) -> Result<Vec<Vec<Sample>>, AdapterError> {
    let mut parts = Vec::new();
    let mut part = Vec::new();
    let mut bytes = 0;
    for sample in samples {
        let size = sample.encoded_len() + 10;
        if size > max_bytes {
            return Err(AdapterError::LimitExceeded {
                limit: "max_batch_bytes",
                value: max_bytes as u64,
            });
        }
        if part.len() >= max_samples || bytes + size > max_bytes {
            parts.push(std::mem::take(&mut part));
            bytes = 0;
        }
        bytes += size;
        part.push(sample);
    }
    if !part.is_empty() {
        parts.push(part);
    }
    Ok(parts)
}

/// Each stream's samples of a batch, in their order. Streams are few.
fn by_stream(
    registry: &Registry,
    samples: Vec<Sample>,
) -> Result<Vec<(String, Vec<Sample>)>, AdapterError> {
    let mut groups: Vec<(String, Vec<Sample>)> = Vec::new();
    for sample in samples {
        let Some(stream) = registry.stream_of(&sample.signal) else {
            return Err(AdapterError::Conflict {
                resource: sample.signal,
                detail: "the file has samples of a signal it does not declare".to_owned(),
            });
        };
        match groups.iter_mut().find(|(name, _)| name == stream) {
            Some((_, group)) => group.push(sample),
            None => groups.push((stream.to_owned(), vec![sample])),
        }
    }
    Ok(groups)
}

/// Imports everything `source` holds through the public API. Safe to repeat: a
/// sample already held is not stored again, however old (see `wire::present`).
///
/// At most one source batch and one daemon batch are in memory, plus a line per
/// signal. A repeat holds to that too: it queries what it may hold.
///
/// # Errors
/// [`AdapterError`]: damage in the file, with its row, stops the import where it
/// is, after everything before that row is stored. Repeating after a fix adds
/// only what is missing.
pub async fn import<S: SampleSource + ?Sized>(
    channel: Channel,
    source: &mut S,
    options: &ImportOptions,
) -> Result<ImportReport, AdapterError> {
    let limits = negotiate(channel.clone(), &[Capability::AppendSamples]).await?;
    let published = usize::try_from(limits.max_batch_samples)
        .unwrap_or(usize::MAX)
        .max(1);
    let max_samples = options
        .batch_samples
        .map_or(published, |n| n.clamp(1, published));
    // Room for the request's own fields.
    let max_bytes = usize::try_from(limits.max_batch_bytes)
        .unwrap_or(usize::MAX)
        .saturating_sub(1_024)
        .max(1);
    let mut registry = Registry::new(channel.clone());
    registry.apply(&source.catalog()?).await?;
    let newest = newest_times(&channel).await?;
    let mut appender = Appender::open(channel.clone(), options.level, options.max_retries).await?;
    let mut report = ImportReport::default();
    while let Some(SourceBatch { signals, samples }) = source.next_batch()? {
        report.samples_read += samples.len() as u64;
        for signal in &signals {
            registry.ensure_signal(signal).await?;
        }
        for (stream, group) in by_stream(&registry, samples)? {
            let (group, present) = without_present(&channel, &newest, group).await?;
            report.samples_present += present;
            for part in split(group, max_samples, max_bytes)? {
                report.samples_sent += part.len() as u64;
                appender.send(&stream, part).await?;
            }
        }
    }
    report.resources_created = registry.created;
    report.samples_duplicate = appender.duplicates;
    report.batches = appender.batches;
    report.throttled = appender.throttled;
    report.retried = appender.retried;
    appender.close().await?;
    Ok(report)
}
