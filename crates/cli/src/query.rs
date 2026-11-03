//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl query`: `QuerySamples`, printed as it streams.

use std::io::Write;
use std::time::{SystemTime, UNIX_EPOCH};

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, QualitySubstatus, Sample, Value};
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{
    Aggregate, AggregateFunction, Aggregation, QuerySamplesRequest, SampleOrder,
};
use prost_types::Timestamp;
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};
use crate::time::{duration, format, instant};

fn function(word: &str) -> Result<AggregateFunction, CliError> {
    Ok(match word {
        "latest" => AggregateFunction::Latest,
        "first" => AggregateFunction::First,
        "last" => AggregateFunction::Last,
        "count" => AggregateFunction::Count,
        "min" => AggregateFunction::Min,
        "max" => AggregateFunction::Max,
        "average" => AggregateFunction::Average,
        "sum" => AggregateFunction::Sum,
        "rate" => AggregateFunction::Rate,
        _ => return Err(usage("--agg is latest, first, last, count, min, max, ...")),
    })
}

pub(crate) fn timestamp(word: &str) -> Result<Timestamp, CliError> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    instant(word, now.as_nanos() as i128)
        .map(|(seconds, nanos)| Timestamp { seconds, nanos })
        .ok_or_else(|| usage("T is RFC 3339 UTC, Unix seconds, now or now-D"))
}

pub(crate) fn window(word: &str) -> Result<prost_types::Duration, CliError> {
    let nanos = duration(word).ok_or_else(|| usage("--window is like 500ms, 10s, 5m, 1h"))?;
    Ok(prost_types::Duration {
        seconds: i64::try_from(nanos / 1_000_000_000).unwrap_or(i64::MAX),
        nanos: (nanos % 1_000_000_000) as i32,
    })
}

/// `parent` and `filter` for a TARGET.
pub(crate) fn selection(target: &str) -> (String, String) {
    if target == "all" {
        return ("entities/-".to_owned(), String::new());
    }
    if let Some((entity, signal)) = target.split_once("/signals/") {
        return (entity.to_owned(), format!("id = \"{signal}\""));
    }
    let name = target.strip_prefix("entities/").unwrap_or(target);
    (format!("entities/{name}"), String::new())
}

/// The request and the `--limit`.
fn request(words: &[&str]) -> Result<(QuerySamplesRequest, Option<u64>), CliError> {
    let Some((target, flags)) = words.split_first() else {
        return Err(usage("query TARGET [--from T] [--to T] ... (see --help)"));
    };
    let (parent, filter) = selection(target);
    let mut r = QuerySamplesRequest {
        parent,
        filter,
        ..QuerySamplesRequest::default()
    };
    let (mut interval, mut aggregation, mut limit) =
        (Interval::default(), Aggregation::default(), None);
    let (mut windowed, mut flags) = (false, flags.iter());
    while let Some(flag) = flags.next() {
        let word = *flags.next().ok_or_else(|| usage("a flag needs a value"))?;
        match *flag {
            "--from" => interval.start_time = Some(timestamp(word)?),
            "--to" => interval.end_time = Some(timestamp(word)?),
            "--where" => r.sample_filter = word.to_owned(),
            "--select" => r.filter = word.to_owned(),
            "--agg" => aggregation.aggregate_function = function(word)? as i32,
            "--window" => {
                aggregation.window = Some(window(word)?);
                windowed = true;
            }
            "--order" => {
                r.order = match word {
                    "signal" => SampleOrder::BySignal,
                    "time" => SampleOrder::ByTime,
                    _ => return Err(usage("--order is signal or time")),
                } as i32;
            }
            "--limit" => limit = Some(word.parse().map_err(|_| usage("--limit: a number"))?),
            "--resume" => r.resume_token = word.to_owned(),
            _ => return Err(usage("query: unknown flag (see --help)")),
        }
    }
    if interval != Interval::default() {
        r.interval = Some(interval);
    }
    if aggregation.aggregate_function != 0 {
        r.aggregation = Some(aggregation);
    } else if windowed {
        return Err(usage("--window needs --agg"));
    }
    Ok((r, limit))
}

fn show_time(t: Option<Timestamp>) -> String {
    t.map_or_else(|| "-".to_owned(), |t| format(t.seconds, t.nanos))
}

fn show_value(value: Option<Value>) -> String {
    match value.and_then(|v| v.kind) {
        Some(Kind::BoolValue(v)) => v.to_string(),
        Some(Kind::Int32Value(v)) => v.to_string(),
        Some(Kind::Int64Value(v)) | Some(Kind::Uint32Value(v)) => v.to_string(),
        Some(Kind::Uint64Value(v)) => (v as u64).to_string(),
        Some(Kind::FloatValue(v)) => v.to_string(),
        Some(Kind::DoubleValue(v)) => v.to_string(),
        Some(Kind::StringValue(v)) => v,
        Some(Kind::BytesValue(v)) => v.iter().map(|b| format!("{b:02x}")).collect(),
        Some(Kind::EnumValue(v)) => v.number.to_string(),
        None => "-".to_owned(),
    }
}

fn show_quality(q: Option<Quality>) -> String {
    let q = q.unwrap_or_default();
    let severity = QualitySeverity::try_from(q.severity)
        .unwrap_or(QualitySeverity::Unspecified)
        .as_str_name()
        .trim_start_matches("QUALITY_SEVERITY_");
    match QualitySubstatus::try_from(q.substatus) {
        Ok(QualitySubstatus::Unspecified) | Err(_) => severity.to_owned(),
        Ok(s) => {
            let detail = s.as_str_name().trim_start_matches("QUALITY_SUBSTATUS_");
            format!("{severity}/{detail}")
        }
    }
}

pub(crate) fn print_sample(out: &mut impl Write, s: Sample) -> std::io::Result<()> {
    let time = show_time(s.observe_time);
    let (value, quality) = (show_value(s.value), show_quality(s.quality));
    writeln!(
        out,
        "{time} {} {value} {quality} seq={}",
        s.signal, s.sequence
    )
}

fn print_aggregate(out: &mut impl Write, a: Aggregate) -> std::io::Result<()> {
    let window = a.window.unwrap_or_default();
    let (start, end) = (show_time(window.start_time), show_time(window.end_time));
    let value = show_value(a.value);
    writeln!(
        out,
        "{start} {end} {} {value} count={}",
        a.signal, a.sample_count
    )
}

/// Runs `query`, printing results as they arrive. With `--limit`, stops at the
/// first response that reaches it and prints `resume_token:`, which `--resume`
/// takes. Results therefore can run a page past the limit, never short of it.
pub(crate) async fn query(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let (request, limit) = request(words)?;
    let method = "QueryService.QuerySamples";
    let mut client = QueryServiceClient::new(channel);
    let stream = client.query_samples(request).await;
    let mut stream = stream.map_err(call(method))?.into_inner();
    let mut printed = 0u64;
    while let Some(page) = stream.message().await.map_err(call(method))? {
        printed += (page.samples.len() + page.aggregates.len()) as u64;
        for s in page.samples {
            print_sample(out, s)?;
        }
        for a in page.aggregates {
            print_aggregate(out, a)?;
        }
        if limit.is_some_and(|l| printed >= l) {
            writeln!(out, "resume_token: {}", page.resume_token)?;
            break;
        }
    }
    Ok(())
}
