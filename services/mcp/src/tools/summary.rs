//! What the daemon's aggregates say of one signal over one interval. No sample crosses
//! to this process but a first and a last.
//!
//! These are several reads and not one snapshot: on a signal still receiving samples,
//! an open interval's `sample_count` and `max` can see different data. Name a closed
//! interval when that matters.

use norn_protocol::common::ValueType;
use norn_protocol::google::r#type::Interval;
use norn_protocol::query::{
    Aggregate, AggregateFunction, Aggregation, QuerySamplesRequest, SampleOrder,
};
use norn_protocol::signal::Signal;
use serde_json::{Map, Value, json};

use crate::daemon::Call;
use crate::error::ToolError;
use crate::render;

const METHOD: &str = "QueryService.QuerySamples";

/// A signal's statistics, and the numbers among them for arithmetic between periods.
pub(super) type Statistics = (Map<String, Value>, Vec<(&'static str, f64)>);

/// One signal over one interval: which samples, and what to read them as.
pub(super) struct Reader<'a> {
    pub(super) call: &'a Call,
    pub(super) signal: &'a Signal,
    pub(super) interval: Option<Interval>,
    pub(super) filter: &'a str,
}

impl Reader<'_> {
    fn request(&self) -> QuerySamplesRequest {
        let (entity, id) = self.signal.name.split_once("/signals/").unwrap_or_default();
        QuerySamplesRequest {
            parent: entity.to_owned(),
            filter: format!("id = \"{id}\""),
            interval: self.interval,
            sample_filter: self.filter.to_owned(),
            ..QuerySamplesRequest::default()
        }
    }

    async fn reduce(&self, f: AggregateFunction) -> Result<Option<Aggregate>, ToolError> {
        let mut request = self.request();
        request.aggregation = Some(Aggregation {
            aggregate_function: f as i32,
            window: None,
        });
        let mut client = self.call.query();
        let mut stream = self.call.rpc(METHOD, client.query_samples(request)).await?;
        let first = self.call.rpc(METHOD, async {
            stream.message().await.map(tonic::Response::new)
        });
        Ok(first
            .await?
            .and_then(|page| page.aggregates.into_iter().next()))
    }

    /// The earliest sample, whose time no aggregate gives. Reads its first page only.
    async fn earliest(&self) -> Result<Option<Value>, ToolError> {
        let mut request = self.request();
        request.order = SampleOrder::BySignal as i32;
        let mut client = self.call.query();
        let mut stream = self.call.rpc(METHOD, client.query_samples(request)).await?;
        let first = self.call.rpc(METHOD, async {
            stream.message().await.map(tonic::Response::new)
        });
        let first = first
            .await?
            .and_then(|page| page.samples.into_iter().next());
        Ok(first.map(|s| {
            let (value, _) = render::value(s.value.as_ref());
            json!({"time": render::when(s.observe_time), "value": value})
        }))
    }
}

fn numeric(signal: &Signal) -> bool {
    use ValueType as T;
    matches!(
        ValueType::try_from(signal.value_type),
        Ok(T::Int32 | T::Int64 | T::Uint32 | T::Uint64 | T::Float | T::Double)
    )
}

/// One signal's statistics.
pub(super) async fn statistics(r: &Reader<'_>) -> Result<Statistics, ToolError> {
    let mut row = Map::new();
    let mut numbers = Vec::new();
    let count = r.reduce(AggregateFunction::Count).await?;
    let count = count.map_or(0, |a| a.sample_count);
    row.insert("sample_count".to_owned(), json!(count));
    numbers.push(("sample_count", count as f64));
    if count == 0 {
        return Ok((row, numbers));
    }
    let mut functions = vec![
        ("min", AggregateFunction::Min),
        ("max", AggregateFunction::Max),
    ];
    if numeric(r.signal) {
        functions.push(("average", AggregateFunction::Average));
        functions.push(("sum", AggregateFunction::Sum));
    }
    for (name, function) in functions {
        if let Some(a) = r.reduce(function).await? {
            row.insert(name.to_owned(), render::value(a.value.as_ref()).0);
            if let Some(n) = render::number(a.value.as_ref()) {
                numbers.push((name, n));
            }
        }
    }
    if let Some(first) = r.earliest().await? {
        row.insert("first".to_owned(), first);
    }
    if let Some(a) = r.reduce(AggregateFunction::Latest).await? {
        let time = render::when(a.window.unwrap_or_default().start_time);
        let value = render::value(a.value.as_ref()).0;
        row.insert("last".to_owned(), json!({"time": time, "value": value}));
    }
    Ok((row, numbers))
}
