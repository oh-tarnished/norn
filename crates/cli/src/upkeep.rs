//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `compact` and `retention`, long-running operations, polled; `verify` is in
//! its own module and polls the same way.

use std::io::Write;
use std::time::Duration;

use norn_protocol::google::longrunning::operations_client::OperationsClient;
use norn_protocol::google::longrunning::{GetOperationRequest, Operation, operation};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{
    ApplyStreamRetentionRequest, ApplyStreamRetentionResponse, CompactStreamRequest,
    CompactStreamResponse,
};
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};
use crate::time::format;

/// How often an operation is polled. Each poll is one call, so `--timeout`
/// bounds a poll and not the operation.
const POLL: Duration = Duration::from_millis(100);

/// `streams/{id}` for a bare id or a full name.
pub(crate) fn stream_name(name: &str) -> String {
    if name.starts_with("streams/") {
        name.to_owned()
    } else {
        format!("streams/{name}")
    }
}

/// Polls `op` until it is done, then decodes its response.
pub(crate) async fn finish<M: prost::Name + Default>(
    channel: Channel,
    op: Operation,
) -> Result<M, CliError> {
    let mut client = OperationsClient::new(channel);
    let mut op = op;
    while !op.done {
        tokio::time::sleep(POLL).await;
        let request = GetOperationRequest {
            name: op.name.clone(),
        };
        let polled = client.get_operation(request).await;
        op = polled
            .map_err(call("Operations.GetOperation"))?
            .into_inner();
    }
    let method = "Operations.GetOperation";
    match op.result {
        Some(operation::Result::Response(any)) => any.to_msg().map_err(|_| CliError::Malformed {
            method,
            field: "response",
        }),
        Some(operation::Result::Error(e)) => {
            let code = tonic::Code::from_i32(e.code);
            let status = tonic::Status::new(code, e.message);
            Err(CliError::Call { method, status })
        }
        None => Err(CliError::Malformed {
            method,
            field: "result",
        }),
    }
}

/// `nornctl compact STREAM`: waits for the whole stream, and prints what changed.
pub(crate) async fn compact(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let [stream] = words else {
        return Err(usage("compact STREAM"));
    };
    let name = stream_name(stream);
    let request = CompactStreamRequest {
        name: name.clone(),
        interval: None,
    };
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.compact_stream(request).await;
    let op = op
        .map_err(call("StreamService.CompactStream"))?
        .into_inner();
    let r: CompactStreamResponse = finish(channel, op).await?;
    if r.original_chunk_count == 0 && r.skipped_segment_count == 0 {
        // Compaction rewrites sealed segments only. `chunks=0->0` read as if
        // something had been checked and found tidy, when there was nothing to
        // look at: say which.
        return Ok(writeln!(
            out,
            "{name} nothing to compact: no sealed segment is waiting. Compaction rewrites \
             sealed segments only; the open one seals when it is full, when it reaches \
             storage.max_segment_open_time, or when the daemon stops"
        )?);
    }
    Ok(writeln!(
        out,
        "{name} chunks={}->{} bytes={}->{} skipped_segments={}",
        r.original_chunk_count,
        r.compacted_chunk_count,
        r.original_bytes,
        r.compacted_bytes,
        r.skipped_segment_count
    )?)
}

/// `nornctl retention apply STREAM [--dry-run]`.
pub(crate) async fn apply(
    channel: Channel,
    name: String,
    dry_run: bool,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let request = ApplyStreamRetentionRequest {
        name: name.clone(),
        validate_only: dry_run,
    };
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.apply_stream_retention(request).await;
    let op = op.map_err(call("StreamService.ApplyStreamRetention"))?;
    let r: ApplyStreamRetentionResponse = finish(channel, op.into_inner()).await?;
    let verb = if r.applied { "removed" } else { "would remove" };
    let time =
        |t: Option<prost_types::Timestamp>| t.map_or("-".into(), |t| format(t.seconds, t.nanos));
    writeln!(
        out,
        "{name} {verb} segments={} samples={} bytes={} retained_bytes={} cutoff={}",
        r.removed_segment_count,
        r.removed_sample_count,
        r.removed_bytes,
        r.retained_bytes,
        time(r.cutoff_time)
    )?;
    for s in r.removed_segments {
        writeln!(
            out,
            "{} samples={} bytes={} from={} to={}",
            s.segment,
            s.sample_count,
            s.byte_count,
            time(s.oldest_sample_time),
            time(s.newest_sample_time)
        )?;
    }
    Ok(())
}
