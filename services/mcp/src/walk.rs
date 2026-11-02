//! Walking `QuerySamples` from a cursor, stopping where a visitor or the deadline says.

use norn_protocol::common::Sample;
use norn_protocol::query::{Aggregate, QuerySamplesRequest, QuerySamplesResponse};
use tokio::time::timeout_at;
use tonic::Streaming;

use crate::cursor::{Cursor, Key, Order};
use crate::daemon::Call;
use crate::error::ToolError;
use crate::render;

const METHOD: &str = "QueryService.QuerySamples";

/// One result of a query.
#[derive(Debug)]
pub(crate) enum Item {
    Sample(Sample),
    Aggregate(Aggregate),
}

impl Item {
    fn key(&self) -> Key {
        match self {
            Self::Sample(s) => render::sample_key(s),
            Self::Aggregate(a) => render::aggregate_key(a),
        }
    }
}

fn items(page: QuerySamplesResponse) -> impl Iterator<Item = Item> {
    let samples = page.samples.into_iter().map(Item::Sample);
    samples.chain(page.aggregates.into_iter().map(Item::Aggregate))
}

async fn open(
    call: &Call,
    request: QuerySamplesRequest,
) -> Result<Streaming<QuerySamplesResponse>, ToolError> {
    let mut client = call.query();
    call.rpc(METHOD, client.query_samples(request)).await
}

/// Feeds `visit` every item after `cursor`, in `order`, until it returns false for
/// one or the results end. Memory: one daemon page.
///
/// Returns the cursor to continue from, or `None` when the results ran out. A visitor
/// refusing an item leaves that item for the continuation; so the cursor exists only
/// if something remains, and "more" is never a guess. Reaching the deadline with
/// progress made is also a continuation; without any, it is an error.
///
/// # Errors
/// What the daemon returned, such as `DATA_LOSS`. Rows already visited stay exact,
/// since a page is read whole before any of it is sent.
pub(crate) async fn walk(
    call: &Call,
    mut request: QuerySamplesRequest,
    order: Order,
    cursor: &Cursor,
    mut visit: impl FnMut(Item) -> bool,
) -> Result<Option<Cursor>, ToolError> {
    request.resume_token.clone_from(&cursor.r);
    let mut stream = open(call, request).await?;
    let mut page = cursor.r.clone();
    let mut last = None;
    let stop = |page: &str, last: Option<Key>| Cursor {
        r: page.to_owned(),
        a: last.or_else(|| cursor.a.clone()),
        ..cursor.clone()
    };
    loop {
        let message = match timeout_at(call.deadline, stream.message()).await {
            Ok(Ok(Some(message))) => message,
            Ok(Ok(None)) => return Ok(None),
            Ok(Err(status)) => return Err(ToolError::daemon(METHOD, &status)),
            Err(_) if last.is_some() => return Ok(Some(stop(&page, last))),
            Err(_) => return Err(ToolError::deadline()),
        };
        let token = message.resume_token.clone();
        for item in items(message) {
            let key = item.key();
            if cursor.given(&key, order) {
                continue;
            }
            if !visit(item) {
                return Ok(Some(stop(&page, last)));
            }
            last = Some(key);
        }
        page = token;
    }
}
