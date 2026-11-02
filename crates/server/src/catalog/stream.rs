//! `norn.stream.v1.StreamService`. Compaction and export answer `UNIMPLEMENTED`.

use std::time::Duration;

use norn_metadata::{Retention, StreamPatch, StreamRecord, StreamSpec};
use norn_protocol::google::longrunning::Operation;
use norn_protocol::stream::retention_policy::Limit;
use norn_protocol::stream::stream_service_server::{SERVICE_NAME, StreamService};
use norn_protocol::stream::{
    CompactStreamRequest, CreateStreamRequest, DeleteStreamRequest, ExportStreamRequest,
    GetStreamRequest, ListStreamsRequest, ListStreamsResponse, RetentionPolicy, Stream,
    StreamStats, UpdateStreamRequest,
};
use tonic::{Request, Response, Status};

use super::query::{Mask, after_token, matches, page_size, page_token, parse_filter};
use super::{
    Catalog, annotation_map, annotations, etag, expected, metadata_status, names, write_mode,
};
use crate::convert::time_to_wire;
use crate::status::bad_field;
use crate::unimplemented::not_implemented;

const FILTERABLE: [&str; 1] = ["display_name"];
const MUTABLE: [&str; 3] = ["display_name", "retention", "annotations"];

fn retention_from_wire(policy: Option<RetentionPolicy>) -> Result<Retention, Status> {
    match policy.and_then(|p| p.limit) {
        None => Ok(Retention::Forever),
        Some(Limit::MaxAge(age)) => Duration::try_from(age)
            .map(Retention::MaxAge)
            .map_err(|_| bad_field("retention.max_age", "must be positive")),
        Some(Limit::MaxBytes(bytes)) => u64::try_from(bytes)
            .ok()
            .filter(|n| *n > 0)
            .map(Retention::MaxBytes)
            .ok_or_else(|| bad_field("retention.max_bytes", "must be positive")),
    }
}

fn retention_to_wire(r: Retention) -> Option<RetentionPolicy> {
    let limit = match r {
        Retention::MaxAge(age) => Limit::MaxAge(age.try_into().ok()?),
        Retention::MaxBytes(n) => Limit::MaxBytes(i64::try_from(n).unwrap_or(i64::MAX)),
        _ => return None,
    };
    Some(RetentionPolicy { limit: Some(limit) })
}

impl Catalog {
    fn stream_to_wire(&self, r: &StreamRecord) -> Stream {
        let stats = self.ingest.stats(r.descriptor.id);
        let wide = |n: u64| i64::try_from(n).unwrap_or(i64::MAX);
        Stream {
            name: format!("streams/{}", r.descriptor.name),
            display_name: r.display_name.clone(),
            retention: retention_to_wire(r.retention),
            annotations: annotation_map(&r.descriptor.metadata),
            stats: Some(StreamStats {
                sample_count: wide(stats.samples),
                buffered_sample_count: wide(stats.buffered),
                corrupt_sample_count: wide(stats.corrupt_samples),
                stored_bytes: wide(stats.stored_bytes),
                oldest_sample_time: stats.time_range.map(|(a, _)| time_to_wire(a)),
                newest_sample_time: stats.time_range.map(|(_, b)| time_to_wire(b)),
            }),
            create_time: Some(time_to_wire(r.create_time)),
            update_time: Some(time_to_wire(r.update_time)),
            etag: etag(r.revision),
        }
    }
}

#[tonic::async_trait]
impl StreamService for Catalog {
    async fn create_stream(
        &self,
        request: Request<CreateStreamRequest>,
    ) -> Result<Response<Stream>, Status> {
        let r = request.into_inner();
        let stream = r.stream.ok_or_else(|| bad_field("stream", "is required"))?;
        let map = annotations(&self.registry, stream.annotations)?;
        let retention = retention_from_wire(stream.retention)?;
        let spec = StreamSpec::new(stream.display_name, retention, map);
        let (id, mode, now) = (r.stream_id, write_mode(r.validate_only), self.clock.now());
        let made = self.mutate(move |g| g.create_stream(&id, spec, now, mode));
        Ok(Response::new(self.stream_to_wire(&made.await?)))
    }

    async fn get_stream(
        &self,
        request: Request<GetStreamRequest>,
    ) -> Result<Response<Stream>, Status> {
        let name = request.into_inner().name;
        let record = self.registry.stream(names::stream(&name)?);
        let record = record.map_err(|e| metadata_status(&e))?;
        Ok(Response::new(self.stream_to_wire(&record)))
    }

    async fn list_streams(
        &self,
        request: Request<ListStreamsRequest>,
    ) -> Result<Response<ListStreamsResponse>, Status> {
        let r = request.into_inner();
        let size = page_size(r.page_size)?;
        let clauses = parse_filter(&r.filter, &FILTERABLE)?;
        let after = after_token(&r.page_token, "", &r.filter)?;
        let mut page = self.registry.list_streams(after, size + 1, |s| {
            matches(&clauses, |field| match field {
                "display_name" => Some(s.display_name.clone()),
                other => other
                    .strip_prefix("annotations.")
                    .and_then(|k| s.descriptor.metadata.get(k))
                    .map(str::to_owned),
            })
        });
        let more = page.len() > size;
        page.truncate(size);
        let next_page_token = match page.last() {
            Some(last) if more => page_token("", &r.filter, last.descriptor.name.as_str()),
            _ => String::new(),
        };
        Ok(Response::new(ListStreamsResponse {
            streams: page.iter().map(|s| self.stream_to_wire(s)).collect(),
            next_page_token,
        }))
    }

    async fn update_stream(
        &self,
        request: Request<UpdateStreamRequest>,
    ) -> Result<Response<Stream>, Status> {
        let r = request.into_inner();
        let stream = r.stream.ok_or_else(|| bad_field("stream", "is required"))?;
        let id = names::stream(&stream.name)?.to_owned();
        let mask = Mask::parse(r.update_mask, &MUTABLE)?;
        let name = mask.wants("display_name", !stream.display_name.is_empty());
        let keep = mask.wants("retention", stream.retention.is_some());
        let retention = keep
            .then(|| retention_from_wire(stream.retention))
            .transpose()?;
        let notes = mask.wants("annotations", !stream.annotations.is_empty());
        let notes = notes
            .then(|| annotations(&self.registry, stream.annotations))
            .transpose()?;
        let patch = StreamPatch::new(name.then_some(stream.display_name), retention, notes);
        let (want, mode, now) = (
            expected(&stream.etag),
            write_mode(r.validate_only),
            self.clock.now(),
        );
        let done = self.mutate(move |g| g.update_stream(&id, patch, want, now, mode));
        Ok(Response::new(self.stream_to_wire(&done.await?)))
    }

    async fn delete_stream(
        &self,
        request: Request<DeleteStreamRequest>,
    ) -> Result<Response<()>, Status> {
        let r = request.into_inner();
        let id = names::stream(&r.name)?.to_owned();
        let (want, mode) = (expected(&r.etag), write_mode(r.validate_only));
        self.mutate(move |g| g.delete_stream(&id, want, mode))
            .await?;
        Ok(Response::new(()))
    }

    async fn compact_stream(
        &self,
        _: Request<CompactStreamRequest>,
    ) -> Result<Response<Operation>, Status> {
        let capability = Some("CAPABILITY_COMPACTION");
        Err(not_implemented(SERVICE_NAME, "CompactStream", capability))
    }

    async fn export_stream(
        &self,
        _: Request<ExportStreamRequest>,
    ) -> Result<Response<Operation>, Status> {
        let capability = Some("CAPABILITY_EXPORT");
        Err(not_implemented(SERVICE_NAME, "ExportStream", capability))
    }
}
