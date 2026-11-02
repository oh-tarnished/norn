//! `norn.query.v1.QueryService`: `QuerySamples`.

pub(crate) mod reply;
pub(crate) mod request;

use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use norn_metadata::Registry;
use norn_protocol::query::query_service_server::QueryService;
use norn_protocol::query::{QuerySamplesRequest, QuerySamplesResponse};
use norn_runtime::query::{Charge, Engine, Query, Result as QueryResult};
use tokio::sync::mpsc;
use tonic::codegen::tokio_stream::Stream;
use tonic::{Request, Response, Status};

/// Pages finished and waiting for the client: enough that reading and writing
/// overlap, few enough that a stalled client pins two pages.
const QUEUED_PAGES: usize = 2;

type Item = Result<(QuerySamplesResponse, Charge), Status>;

/// One call's responses. Each stays charged to the query budget until tonic takes
/// it to encode.
pub struct Pages(mpsc::Receiver<Item>);

impl Stream for Pages {
    type Item = Result<QuerySamplesResponse, Status>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        self.0
            .poll_recv(cx)
            .map(|item| item.map(|r| r.map(|(page, _charge)| page)))
    }
}

/// The QueryService implementation.
///
/// Each call is one task that pulls a page on the blocking pool only once the
/// client has room for it, so a client that stops reading holds its queued pages'
/// charge and no thread. The task ends when the query does or the client goes.
/// Memory: the query budget, through `norn-query`. Concurrency: `Clone`, shared by
/// tonic; any number of calls run, up to what the budget lets through.
#[derive(Debug, Clone)]
pub struct Queries {
    registry: Arc<Registry>,
    engine: Arc<Engine>,
}

impl Queries {
    /// A service selecting signals from `registry` and reading through `engine`.
    #[must_use]
    pub fn new(registry: Arc<Registry>, engine: Arc<Engine>) -> Self {
        Self { registry, engine }
    }
}

/// Pulls pages until the query ends or the client leaves.
async fn pump(mut query: Query, print: u64, out: mpsc::Sender<Item>) {
    loop {
        // Waits for the client, then does the work: nothing is read ahead of it.
        let Ok(permit) = out.reserve().await else {
            return;
        };
        let pulled = tokio::task::spawn_blocking(move || {
            let page: QueryResult<_> = query.next_page();
            (query, page)
        })
        .await;
        let Ok((back, page)) = pulled else {
            permit.send(Err(Status::internal("a query panicked")));
            return;
        };
        query = back;
        match page {
            Ok(Some(page)) => permit.send(reply::page(page, print)),
            Ok(None) => return,
            Err(e) => {
                permit.send(Err(reply::status(&e)));
                return;
            }
        }
    }
}

#[tonic::async_trait]
impl QueryService for Queries {
    type QuerySamplesStream = Pages;

    async fn query_samples(
        &self,
        request: Request<QuerySamplesRequest>,
    ) -> Result<Response<Pages>, Status> {
        let request = request.into_inner();
        let print = request::fingerprint(&request);
        let (registry, engine) = (Arc::clone(&self.registry), Arc::clone(&self.engine));
        // Listing signals, and a budget wait, must not sit on an async thread.
        let query = tokio::task::spawn_blocking(move || {
            let spec = request::spec(&registry, request, print)?;
            engine.open(spec).map_err(|e| reply::status(&e))
        })
        .await
        .map_err(|_| Status::internal("a query panicked"))??;
        let (out, pages) = mpsc::channel(QUEUED_PAGES);
        tokio::spawn(pump(query, print, out));
        Ok(Response::new(Pages(pages)))
    }
}
