//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! An operation as `google.longrunning.Operation`, with typed Any payloads.

use norn_protocol::admin::{Backup, CreateBackupMetadata};
use norn_protocol::common::ErrorReason;
use norn_protocol::google::longrunning::{Operation, operation};
use norn_protocol::stream::{
    ApplyStreamRetentionMetadata, ApplyStreamRetentionResponse, CompactStreamMetadata,
    CompactStreamResponse, DamagedChunk, RemovedSegment, VerifyStreamMetadata,
    VerifyStreamResponse,
};
use norn_runtime::maintenance::{
    self, BackupReport, BackupSpec, Failure, Outcome, Request, RetentionReport, VerifyReport,
};
use prost_types::Any;
use tonic::{Code, Status};

use crate::convert::time_to_wire;
use crate::status::{error_info, to_rpc};

fn wide(n: u64) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

fn pack<M: prost::Name>(message: &M) -> Option<Any> {
    Any::from_msg(message).ok()
}

fn metadata(op: &maintenance::Operation) -> Option<Any> {
    let create_time = Some(time_to_wire(op.create_time));
    let end_time = op.ending().map(|(at, _)| time_to_wire(at));
    let (done, total) = op.progress();
    match op.request {
        Request::Compact { .. } => pack(&CompactStreamMetadata {
            create_time,
            end_time,
            chunks_done: wide(done),
            chunks_total: wide(total),
        }),
        Request::Verify => pack(&VerifyStreamMetadata {
            create_time,
            end_time,
            segments_done: wide(done),
            segments_total: wide(total),
        }),
        Request::Backup(_) => pack(&CreateBackupMetadata {
            create_time,
            end_time,
            files_done: wide(done),
            files_total: wide(total),
        }),
        _ => pack(&ApplyStreamRetentionMetadata {
            create_time,
            end_time,
        }),
    }
}

fn verified(r: &VerifyReport) -> VerifyStreamResponse {
    let chunks = r.regions.iter().map(|d| DamagedChunk {
        segment: d.segment.clone(),
        offset: wide(d.offset),
        length: wide(d.len),
        description: d.description.clone(),
    });
    VerifyStreamResponse {
        segment_count: wide(r.segments),
        chunk_count: wide(r.chunks),
        sample_count: wide(r.samples),
        verified_bytes: wide(r.bytes),
        damaged_chunk_count: wide(r.damaged_chunks),
        damaged_sample_count: wide(r.damaged_samples),
        damaged_chunks: chunks.collect(),
    }
}

fn retained(r: &RetentionReport) -> ApplyStreamRetentionResponse {
    let segments = r.removed.iter().map(|s| RemovedSegment {
        segment: s.name.file_name(),
        oldest_sample_time: Some(time_to_wire(s.oldest)),
        newest_sample_time: Some(time_to_wire(s.newest)),
        sample_count: wide(s.samples),
        byte_count: wide(s.bytes),
    });
    ApplyStreamRetentionResponse {
        applied: r.applied,
        cutoff_time: r.cutoff.map(time_to_wire),
        removed_segment_count: wide(r.segments),
        removed_sample_count: wide(r.samples),
        removed_bytes: wide(r.bytes),
        retained_bytes: wide(r.retained_bytes),
        removed_segments: segments.collect(),
    }
}

fn backed_up(r: &BackupReport, spec: &BackupSpec) -> Backup {
    Backup {
        path: spec.to.display().to_string(),
        base_path: spec
            .base
            .as_ref()
            .map_or_else(String::new, |b| b.display().to_string()),
        cut_time: Some(time_to_wire(r.cut)),
        stream_count: wide(r.streams),
        file_count: wide(r.files),
        byte_count: wide(r.bytes),
        copied_file_count: wide(r.copied_files),
        copied_byte_count: wide(r.copied_bytes),
    }
}

fn response(outcome: &Outcome, request: &Request) -> Option<Any> {
    match outcome {
        Outcome::Compacted(t) => pack(&CompactStreamResponse {
            original_chunk_count: wide(t.original_chunks),
            compacted_chunk_count: wide(t.compacted_chunks),
            original_bytes: wide(t.original_bytes),
            compacted_bytes: wide(t.compacted_bytes),
            skipped_segment_count: wide(t.skipped_segments),
        }),
        Outcome::Verified(r) => pack(&verified(r)),
        Outcome::Retained(r) => pack(&retained(r)),
        Outcome::BackedUp(r) => match request {
            Request::Backup(spec) => pack(&backed_up(r, spec)),
            _ => None,
        },
        _ => None,
    }
}

fn failure(f: &Failure) -> Status {
    match f {
        Failure::Failed(why) => error_info(
            Code::Internal,
            ErrorReason::StorageUnavailable,
            why.clone(),
            [],
        ),
        _ => Status::cancelled("the operation was cancelled"),
    }
}

/// `op` as the protocol has it, as of now. Copies.
pub(crate) fn to_wire(op: &maintenance::Operation) -> Operation {
    let ending = op.ending();
    let result = ending.as_ref().map(|(_, r)| match r {
        Ok(outcome) => response(outcome, &op.request).map_or_else(
            || operation::Result::Error(to_rpc(&Status::internal("unencodable result"))),
            operation::Result::Response,
        ),
        Err(f) => operation::Result::Error(to_rpc(&failure(f))),
    });
    Operation {
        name: format!("operations/{}", op.id),
        metadata: metadata(op),
        done: ending.is_some(),
        result,
    }
}
