//! Which samples of a batch the daemon already holds.
//!
//! The daemon's own duplicate check is bounded: below a signal's newest sample it
//! remembers only a recent window, and a resend older than that is kept as late
//! data (docs/reference/storage.md). So an import cannot lean on it alone to be
//! repeatable. It asks first, for just the samples that could be duplicates: at or
//! below their signal's newest time when the import began. A fresh import asks
//! nothing beyond the one `LATEST`.

use std::collections::{BTreeMap, HashMap};

use norn_protocol::common::Sample;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::query::{AggregateFunction, Aggregation, QuerySamplesRequest};
use tonic::transport::Channel;

use crate::{AdapterError, entity_resource, time};

const METHOD: &str = "QueryService.QuerySamples";

/// How many signals of one batch get a query each; past it, one query over the
/// whole time span is cheaper. A batch sorted by time has many signals over a
/// short span, one sorted by signal few over a long one.
const PER_SIGNAL_QUERIES: usize = 8;

/// Each signal's newest time when the import began. One entry per signal the
/// daemon holds samples for.
pub(crate) async fn newest_times(channel: &Channel) -> Result<HashMap<String, i64>, AdapterError> {
    let request = QuerySamplesRequest {
        parent: "entities/-".to_owned(),
        aggregation: Some(Aggregation {
            aggregate_function: AggregateFunction::Latest as i32,
            window: None,
        }),
        ..QuerySamplesRequest::default()
    };
    let mut client = QueryServiceClient::new(channel.clone());
    let replies = client.query_samples(request).await;
    let mut replies = replies.map_err(AdapterError::call(METHOD))?.into_inner();
    let mut newest = HashMap::new();
    while let Some(page) = replies
        .message()
        .await
        .map_err(AdapterError::call(METHOD))?
    {
        for a in page.aggregates {
            let instant = a.window.and_then(|w| w.start_time);
            if let Some(t) = instant.as_ref().and_then(time::to_nanos) {
                newest.insert(a.signal, t);
            }
        }
    }
    Ok(newest)
}

fn key(sample: &Sample) -> Option<(i64, i64)> {
    let t = sample.observe_time.as_ref().and_then(time::to_nanos)?;
    Some((t, sample.sequence))
}

/// `samples` without those the daemon holds, and how many that was. Holds the
/// batch's keys; the query's results are read and dropped as they stream.
pub(crate) async fn without_present(
    channel: &Channel,
    newest: &HashMap<String, i64>,
    samples: Vec<Sample>,
) -> Result<(Vec<Sample>, u64), AdapterError> {
    let suspect = |s: &Sample| {
        let held = newest.get(&s.signal);
        held.zip(key(s)).is_some_and(|(mark, (t, _))| t <= *mark)
    };
    if !samples.iter().any(suspect) {
        return Ok((samples, 0));
    }
    // Signal to sorted (time, sequence), of the suspects alone.
    let mut wanted: BTreeMap<&str, Vec<(i64, i64)>> = BTreeMap::new();
    for s in samples.iter().filter(|s| suspect(s)) {
        if let Some(k) = key(s) {
            wanted.entry(&s.signal).or_default().push(k);
        }
    }
    wanted.values_mut().for_each(|keys| keys.sort_unstable());
    let bounds = |keys: &[(i64, i64)]| Some((keys.first()?.0, keys.last()?.0));
    let mut found: BTreeMap<&str, Vec<(i64, i64)>> = BTreeMap::new();
    let requests: Vec<QuerySamplesRequest> = if wanted.len() <= PER_SIGNAL_QUERIES {
        let each = wanted.iter().filter_map(|(signal, keys)| {
            let (entity, id) = entity_resource(signal)?;
            let filter = format!("id = \"{id}\"");
            let (lo, hi) = bounds(keys)?;
            Some(span(format!("entities/{entity}"), filter, lo, hi))
        });
        each.collect()
    } else {
        let spans = wanted.values().filter_map(|k| bounds(k));
        let lo = spans.clone().map(|(l, _)| l).min().unwrap_or(0);
        let hi = spans.map(|(_, h)| h).max().unwrap_or(0);
        vec![span("entities/-".to_owned(), String::new(), lo, hi)]
    };
    let mut client = QueryServiceClient::new(channel.clone());
    for request in requests {
        let replies = client.query_samples(request).await;
        let mut replies = replies.map_err(AdapterError::call(METHOD))?.into_inner();
        while let Some(page) = replies
            .message()
            .await
            .map_err(AdapterError::call(METHOD))?
        {
            for held in &page.samples {
                let Some((signal, keys)) = wanted.get_key_value(held.signal.as_str()) else {
                    continue;
                };
                let hit = key(held).filter(|k| keys.binary_search(k).is_ok());
                if let Some(k) = hit {
                    found.entry(signal).or_default().push(k);
                }
            }
        }
    }
    found.values_mut().for_each(|keys| keys.sort_unstable());
    let held = |s: &Sample| {
        let keys = found.get(s.signal.as_str());
        keys.zip(key(s))
            .is_some_and(|(keys, k)| keys.binary_search(&k).is_ok())
    };
    let present = samples.iter().filter(|s| held(s)).count() as u64;
    if present == 0 {
        return Ok((samples, 0));
    }
    let kept = samples.iter().filter(|s| !held(s)).cloned().collect();
    Ok((kept, present))
}

/// A raw read of `[lo, hi]`; the interval is half-open, so `hi` is widened.
fn span(parent: String, filter: String, lo: i64, hi: i64) -> QuerySamplesRequest {
    let end = hi.checked_add(1).map(time::from_nanos);
    QuerySamplesRequest {
        parent,
        filter,
        interval: Some(Interval {
            start_time: Some(time::from_nanos(lo)),
            end_time: end,
        }),
        ..QuerySamplesRequest::default()
    }
}
