//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading a selection out of the daemon, into a sink.

use norn_protocol::admin::Capability;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::QuerySamplesRequest;
use norn_protocol::query::query_service_client::QueryServiceClient;
use tonic::transport::Channel;

use crate::wire::{listing, negotiate};
use crate::{AdapterError, DEFAULT_MAX_SIGNALS, SampleSink};

const METHOD: &str = "QueryService.QuerySamples";

/// A response the client will read: above tonic's 4 MiB default, because a
/// sample may be as large as a batch, and still a bound (SPEC §16).
const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;

/// Which samples to export, as `QuerySamples` selects them.
#[derive(Debug, Clone)]
pub struct Selection {
    parent: String,
    filter: String,
    interval: Option<Interval>,
}

impl Selection {
    /// The signals under `parent`, `entities/{entity}` or `entities/-`.
    #[must_use]
    pub fn new(parent: impl Into<String>) -> Self {
        Self {
            parent: parent.into(),
            filter: String::new(),
            interval: None,
        }
    }

    /// An AIP-160 signal filter, as `ListSignals` takes.
    #[must_use]
    pub fn filter(mut self, filter: impl Into<String>) -> Self {
        self.filter = filter.into();
        self
    }

    /// Only samples in this half-open interval of `observe_time`.
    #[must_use]
    pub fn interval(mut self, interval: Interval) -> Self {
        self.interval = Some(interval);
        self
    }
}

/// How an export runs. `Default` allows [`DEFAULT_MAX_SIGNALS`].
#[derive(Debug, Clone)]
pub struct ExportOptions {
    max_signals: usize,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            max_signals: DEFAULT_MAX_SIGNALS,
        }
    }
}

impl ExportOptions {
    /// Refuses a selection of more signals than this.
    #[must_use]
    pub fn max_signals(mut self, max_signals: usize) -> Self {
        self.max_signals = max_signals;
        self
    }
}

/// What an export did.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ExportReport {
    /// Signals selected, with or without samples.
    pub signals: u64,
    /// Samples written.
    pub samples: u64,
    /// Bytes in the finished file.
    pub bytes: u64,
    /// Whether the catalog holds metadata (units, annotations, display names).
    pub has_metadata: bool,
}

/// Exports a selection through the public API, streaming: a slow file stalls the
/// query rather than growing a buffer.
///
/// Samples acknowledged during the export may or may not be in it. A signal made
/// after the catalog was read is not selected unless the query also sees it, and
/// then a format that declares signals up front fails rather than guess.
///
/// # Errors
/// [`AdapterError::Call`], including `DATA_LOSS` for a damaged chunk, which is
/// never skipped; whatever the sink reports. The output is then incomplete, and
/// the caller removes it.
pub async fn export<S: SampleSink + ?Sized>(
    channel: Channel,
    selection: &Selection,
    sink: &mut S,
    options: &ExportOptions,
) -> Result<ExportReport, AdapterError> {
    negotiate(channel.clone(), &[Capability::QueryRange]).await?;
    let catalog = listing::catalog(
        &channel,
        &selection.parent,
        &selection.filter,
        options.max_signals,
    )
    .await?;
    sink.begin(&catalog)?;
    let mut report = ExportReport {
        signals: catalog.signals.len() as u64,
        has_metadata: catalog.has_metadata(),
        ..ExportReport::default()
    };
    let request = QuerySamplesRequest {
        parent: selection.parent.clone(),
        filter: selection.filter.clone(),
        interval: selection.interval,
        order: sink.order() as i32,
        ..QuerySamplesRequest::default()
    };
    let mut client = QueryServiceClient::new(channel).max_decoding_message_size(MAX_RESPONSE_BYTES);
    let replies = client.query_samples(request).await;
    let mut replies = replies.map_err(AdapterError::call(METHOD))?.into_inner();
    while let Some(page) = replies
        .message()
        .await
        .map_err(AdapterError::call(METHOD))?
    {
        report.samples += page.samples.len() as u64;
        sink.write(&page.samples)?;
    }
    report.bytes = sink.finish()?;
    Ok(report)
}
