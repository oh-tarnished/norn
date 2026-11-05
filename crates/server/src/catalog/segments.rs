//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `StreamService.GetSegment` and `ListSegments`: a stream's segment files,
//! read-only (docs/reference/protocol.md, "Inspecting storage").
//!
//! The BASIC view answers from the temporal index on the async thread. FULL
//! reads whole files, so it runs on the blocking pool, and a list page of it
//! is capped at [`MAX_FULL_PAGE`] segments.

use norn_core::StreamId;
use norn_protocol::stream::{
    ChunkColumn, ColumnEncoding, GetSegmentRequest, ListSegmentsRequest, ListSegmentsResponse,
    Segment, SegmentView,
};
use norn_runtime::ingest::{self, LayoutError, SegmentDetail, SegmentInfo};
use tonic::{Code, Status};

use super::query::{after_token, page_size, page_token};
use super::{Catalog, metadata_status, names};
use crate::convert::time_to_wire;
use crate::status::bad_field;

/// The most segments one FULL page reads.
const MAX_FULL_PAGE: usize = 10;

fn wide(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn column(c: ingest::ChunkColumn) -> ChunkColumn {
    match c {
        ingest::ChunkColumn::Timestamps => ChunkColumn::Timestamps,
        ingest::ChunkColumn::Values => ChunkColumn::Values,
        ingest::ChunkColumn::Qualities => ChunkColumn::Qualities,
        ingest::ChunkColumn::Sequences => ChunkColumn::Sequences,
        _ => ChunkColumn::Unspecified,
    }
}

fn to_wire(stream: &str, s: &SegmentInfo, detail: Option<&SegmentDetail>) -> Segment {
    let mut out = Segment {
        name: format!("streams/{stream}/segments/{}", s.id),
        sealed: s.sealed,
        generation: i32::from(s.generation),
        chunk_count: wide(s.chunks),
        sample_count: wide(s.samples),
        size_bytes: wide(s.size_bytes),
        oldest_sample_time: s.time_range.map(|(a, _)| time_to_wire(a)),
        newest_sample_time: s.time_range.map(|(_, b)| time_to_wire(b)),
        ..Segment::default()
    };
    if let Some(d) = detail {
        out.signal_count = i32::try_from(d.signals).unwrap_or(i32::MAX);
        out.decoded_bytes = wide(d.decoded_bytes);
        out.damaged_chunk_count = wide(d.damaged_chunks);
        out.column_encodings = d
            .encodings
            .iter()
            .map(|e| ColumnEncoding {
                column: column(e.column) as i32,
                codec: e.codec.to_owned(),
                zstd_level: i32::from(e.zstd_level),
                chunk_count: wide(e.chunks),
                stored_bytes: wide(e.stored_bytes),
                encoded_bytes: wide(e.encoded_bytes),
            })
            .collect();
    }
    out
}

fn full(view: i32) -> Result<bool, Status> {
    match SegmentView::try_from(view) {
        Ok(SegmentView::Unspecified | SegmentView::Basic) => Ok(false),
        Ok(SegmentView::Full) => Ok(true),
        Err(_) => Err(bad_field("view", "is not a SegmentView")),
    }
}

fn layout_status(e: LayoutError) -> Status {
    match e {
        LayoutError::NotFound => Status::new(Code::NotFound, "no such segment"),
        LayoutError::Storage(e) => Status::new(Code::Internal, e.to_string()),
        _ => Status::new(Code::Internal, "the segment could not be read"),
    }
}

impl Catalog {
    fn segment_stream(&self, name: &str) -> Result<StreamId, Status> {
        let record = self.registry.stream(names::stream(name)?);
        Ok(record.map_err(|e| metadata_status(&e))?.descriptor.id)
    }

    /// Reads segment `id` of `stream`, named `name`, whole, off the async thread.
    async fn read_whole(
        &self,
        stream: StreamId,
        name: &str,
        id: String,
    ) -> Result<Segment, Status> {
        let ingest = std::sync::Arc::clone(&self.ingest);
        let read = tokio::task::spawn_blocking(move || ingest.segment_detail(stream, &id)).await;
        let read = read.map_err(|_| Status::new(Code::Internal, "the read panicked"))?;
        let (info, detail) = read.map_err(layout_status)?;
        Ok(to_wire(name, &info, Some(&detail)))
    }

    pub(super) async fn get_segment_impl(&self, r: GetSegmentRequest) -> Result<Segment, Status> {
        let (parent, id) = r
            .name
            .split_once("/segments/")
            .filter(|(_, id)| !id.is_empty() && !id.contains('/'))
            .ok_or_else(|| bad_field("name", "must be streams/{stream}/segments/{segment}"))?;
        let stream = self.segment_stream(parent)?;
        let named = names::stream(parent)?;
        if full(r.view)? {
            return self.read_whole(stream, named, id.to_owned()).await;
        }
        let found = self
            .ingest
            .segments(stream)
            .into_iter()
            .find(|s| s.id == id);
        let found = found.ok_or_else(|| Status::new(Code::NotFound, "no such segment"))?;
        Ok(to_wire(named, &found, None))
    }

    pub(super) async fn list_segments_impl(
        &self,
        r: ListSegmentsRequest,
    ) -> Result<ListSegmentsResponse, Status> {
        let stream = self.segment_stream(&r.parent)?;
        let id = names::stream(&r.parent)
            .map_err(|_| bad_field("parent", "must be streams/{stream}"))?;
        let whole = full(r.view)?;
        let mut size = page_size(r.page_size)?;
        if whole {
            size = size.min(MAX_FULL_PAGE);
        }
        let after = after_token(&r.page_token, &r.parent, "")?;
        let mut page: Vec<SegmentInfo> = self.ingest.segments(stream);
        page.retain(|s| after.is_none_or(|a| s.id.as_str() > a));
        let more = page.len() > size;
        page.truncate(size);
        let next_page_token = match page.last() {
            Some(last) if more => page_token(&r.parent, "", &last.id),
            _ => String::new(),
        };
        let mut segments = Vec::with_capacity(page.len());
        for s in &page {
            segments.push(match whole {
                true => match self.read_whole(stream, id, s.id.clone()).await {
                    Err(e) if e.code() == Code::NotFound => continue,
                    other => other?,
                },
                false => to_wire(id, s, None),
            });
        }
        Ok(ListSegmentsResponse {
            segments,
            next_page_token,
        })
    }
}
