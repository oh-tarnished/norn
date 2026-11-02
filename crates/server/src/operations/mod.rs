//! `google.longrunning.Operations`, over the maintenance worker's table.

mod wire;

pub(crate) use wire::to_wire;

use std::sync::Arc;
use std::time::Duration;

use norn_protocol::common::ErrorReason;
use norn_protocol::google::longrunning::operations_server::Operations;
use norn_protocol::google::longrunning::{
    CancelOperationRequest, DeleteOperationRequest, GetOperationRequest, ListOperationsRequest,
    ListOperationsResponse, Operation, WaitOperationRequest,
};
use norn_runtime::maintenance::{self, Maintenance, Refused};
use tonic::{Code, Request, Response, Status};

use crate::status::{bad_field, error_info};

/// The longest `WaitOperation` waits, whatever it asks: a held call pins a stream
/// of the connection, and a client can simply ask again.
const MAX_WAIT: Duration = Duration::from_secs(60);
/// How often a waiting call looks.
const WAIT_STEP: Duration = Duration::from_millis(50);
/// `ListOperations`' page size when none is given.
const DEFAULT_PAGE: usize = 100;

/// The Operations service.
///
/// Concurrency: `Clone`, shared by tonic; state is the worker's. Operations live
/// in memory, so a restart forgets them. What they did to the data is durable.
#[derive(Debug, Clone)]
pub struct OperationsService {
    maintenance: Arc<Maintenance>,
}

impl OperationsService {
    /// Serves `maintenance`'s operations.
    #[must_use]
    pub fn new(maintenance: Arc<Maintenance>) -> Self {
        Self { maintenance }
    }

    fn find(&self, name: &str) -> Result<Arc<maintenance::Operation>, Status> {
        let id = id_of(name)?;
        self.maintenance.operation(id).ok_or_else(|| missing(name))
    }
}

fn id_of(name: &str) -> Result<&str, Status> {
    name.strip_prefix("operations/")
        .filter(|id| !id.is_empty())
        .ok_or_else(|| bad_field("name", "must be `operations/{operation}`"))
}

fn missing(name: &str) -> Status {
    let (code, reason) = (Code::NotFound, ErrorReason::ResourceNotFound);
    let message = format!("{name} is not known: it was deleted, evicted, or from an earlier run");
    error_info(code, reason, message, [("resource", name.to_owned())])
}

/// A refused submission as a status.
pub(crate) fn refused(e: Refused) -> Status {
    match e {
        Refused::Stopped => error_info(
            Code::Unavailable,
            ErrorReason::ShuttingDown,
            "the daemon is shutting down",
            [],
        ),
        _ => error_info(
            Code::ResourceExhausted,
            ErrorReason::CapacityExhausted,
            "too many maintenance jobs are waiting; try later",
            [("limit", "max_queued_jobs".to_owned())],
        ),
    }
}

#[tonic::async_trait]
impl Operations for OperationsService {
    async fn list_operations(
        &self,
        request: Request<ListOperationsRequest>,
    ) -> Result<Response<ListOperationsResponse>, Status> {
        let r = request.into_inner();
        if !r.filter.is_empty() {
            return Err(bad_field("filter", "is not supported"));
        }
        let size = usize::try_from(r.page_size)
            .map_err(|_| bad_field("page_size", "must not be negative"))?;
        let size = if size == 0 { DEFAULT_PAGE } else { size };
        let after = (!r.page_token.is_empty()).then_some(r.page_token.as_str());
        let mut page = self.maintenance.operations(after, size.saturating_add(1));
        let more = page.len() > size;
        page.truncate(size);
        let next_page_token = match page.last() {
            Some(last) if more => last.id.clone(),
            _ => String::new(),
        };
        Ok(Response::new(ListOperationsResponse {
            operations: page.iter().map(|op| to_wire(op)).collect(),
            next_page_token,
            unreachable: Vec::new(),
        }))
    }

    async fn get_operation(
        &self,
        request: Request<GetOperationRequest>,
    ) -> Result<Response<Operation>, Status> {
        let op = self.find(&request.into_inner().name)?;
        Ok(Response::new(to_wire(&op)))
    }

    async fn delete_operation(
        &self,
        request: Request<DeleteOperationRequest>,
    ) -> Result<Response<()>, Status> {
        let name = request.into_inner().name;
        let op = self.find(&name)?;
        if !self.maintenance.forget(&op.id) {
            return Err(Status::failed_precondition(format!(
                "{name} is still running"
            )));
        }
        Ok(Response::new(()))
    }

    async fn cancel_operation(
        &self,
        request: Request<CancelOperationRequest>,
    ) -> Result<Response<()>, Status> {
        self.find(&request.into_inner().name)?.cancel();
        Ok(Response::new(()))
    }

    async fn wait_operation(
        &self,
        request: Request<WaitOperationRequest>,
    ) -> Result<Response<Operation>, Status> {
        let r = request.into_inner();
        let op = self.find(&r.name)?;
        let asked = r.timeout.and_then(|t| Duration::try_from(t).ok());
        let mut left = asked.unwrap_or(MAX_WAIT).min(MAX_WAIT);
        while op.ending().is_none() && !left.is_zero() {
            tokio::time::sleep(WAIT_STEP.min(left)).await;
            left = left.saturating_sub(WAIT_STEP);
        }
        Ok(Response::new(to_wire(&op)))
    }
}
