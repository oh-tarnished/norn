//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl stats`: counts and rates per signal and per stream over a window,
//! from `QueryService.ComputeStats`.

use std::collections::{BTreeMap, HashMap};
use std::io::Write;

use norn_protocol::google::r#type::Interval;
use norn_protocol::query::ComputeStatsRequest;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::signal::ListSignalsRequest;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::stream::GetStreamRequest;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use prost_types::Timestamp;
use tonic::transport::Channel;

use crate::CliError;
use crate::query::{selection, timestamp};
use crate::resources::{call, usage};

const USAGE: &str = "stats [TARGET] [--from T] [--to T]";

/// The window's length in seconds.
fn seconds(from: Timestamp, to: Timestamp) -> f64 {
    (to.seconds - from.seconds) as f64 + f64::from(to.nanos - from.nanos) / 1e9
}

/// Which stream each selected signal belongs to.
async fn streams_of(
    channel: Channel,
    parent: &str,
    filter: &str,
) -> Result<HashMap<String, String>, CliError> {
    let mut client = SignalServiceClient::new(channel);
    let (mut map, mut page_token) = (HashMap::new(), String::new());
    loop {
        let request = ListSignalsRequest {
            parent: parent.to_owned(),
            filter: filter.to_owned(),
            page_token,
            ..ListSignalsRequest::default()
        };
        let page = client.list_signals(request).await;
        let page = page
            .map_err(call("SignalService.ListSignals"))?
            .into_inner();
        map.extend(page.signals.into_iter().map(|s| (s.name, s.stream)));
        page_token = page.next_page_token;
        if page_token.is_empty() {
            return Ok(map);
        }
    }
}

/// `nornctl stats [TARGET] [--from T] [--to T]`: TARGET as in `query`, default
/// `all`; the window defaults to the last hour.
pub(crate) async fn stats(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let (target, flags) = match words.split_first() {
        Some((first, rest)) if !first.starts_with('-') => (*first, rest),
        _ => ("all", words),
    };
    let (mut from, mut to) = (timestamp("now-1h")?, timestamp("now")?);
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = flags.next().ok_or_else(|| usage(USAGE))?;
        match *flag {
            "--from" => from = timestamp(word)?,
            "--to" => to = timestamp(word)?,
            _ => return Err(usage(USAGE)),
        }
    }
    let span = seconds(from, to);
    if span <= 0.0 {
        return Err(usage("stats: --from must be before --to"));
    }
    let (parent, filter) = selection(target);
    let owners = streams_of(channel.clone(), &parent, &filter).await?;
    let mut client = QueryServiceClient::new(channel.clone());
    let mut per_stream: BTreeMap<String, (i64, i64)> = BTreeMap::new();
    let mut page_token = String::new();
    loop {
        let request = ComputeStatsRequest {
            parent: parent.clone(),
            filter: filter.clone(),
            interval: Some(Interval {
                start_time: Some(from),
                end_time: Some(to),
            }),
            page_token,
            ..ComputeStatsRequest::default()
        };
        let page = client.compute_stats(request).await;
        let page = page
            .map_err(call("QueryService.ComputeStats"))?
            .into_inner();
        for s in page.signal_stats {
            let stream = owners.get(&s.signal).map_or("-", String::as_str);
            writeln!(
                out,
                "signal {} stream={stream} samples={} rate={:.3}/s",
                s.signal,
                s.sample_count,
                s.sample_count as f64 / span,
            )?;
            let entry = per_stream.entry(stream.to_owned()).or_default();
            entry.0 += 1;
            entry.1 += s.sample_count;
        }
        page_token = page.next_page_token;
        if page_token.is_empty() {
            break;
        }
    }
    let mut streams = StreamServiceClient::new(channel);
    for (name, (signals, samples)) in per_stream {
        let request = GetStreamRequest { name: name.clone() };
        let found = streams.get_stream(request).await;
        let found = found.map_err(call("StreamService.GetStream"))?;
        let held = found.into_inner().stats.unwrap_or_default();
        writeln!(
            out,
            "stream {name} signals={signals} samples={samples} rate={:.3}/s stored_bytes={}",
            samples as f64 / span,
            held.stored_bytes,
        )?;
    }
    Ok(())
}
