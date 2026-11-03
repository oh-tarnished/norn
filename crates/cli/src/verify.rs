//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl verify STREAM`: every stored chunk read and checked, and a check
//! that examined nothing never reported as a pass.

use std::io::Write;

use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{GetStreamRequest, VerifyStreamRequest, VerifyStreamResponse};
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};
use crate::upkeep::{finish, stream_name};

/// `nornctl verify STREAM`. The daemon reads the sealed segments and the
/// chunks already written to the open one; samples still in the write buffers
/// are in no file yet, and are counted as `buffered`.
///
/// Fails, so a script can test it, when a chunk is damaged, and when the
/// stream holds samples yet nothing was read: a check that examined nothing
/// has checked nothing.
pub(crate) async fn verify(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let [stream] = words else {
        return Err(usage("verify STREAM"));
    };
    let name = stream_name(stream);
    let request = VerifyStreamRequest { name: name.clone() };
    let mut client = StreamServiceClient::new(channel.clone());
    let op = client.verify_stream(request).await;
    let op = op.map_err(call("StreamService.VerifyStream"))?.into_inner();
    let r: VerifyStreamResponse = finish(channel, op).await?;
    // Read after the verify, so `held` covers what it could have found.
    let got = client
        .get_stream(GetStreamRequest { name: name.clone() })
        .await;
    let stats = got
        .map_err(call("StreamService.GetStream"))?
        .into_inner()
        .stats
        .unwrap_or_default();
    let held = stats
        .sample_count
        .saturating_add(stats.corrupt_sample_count);
    writeln!(
        out,
        "{name} segments={} chunks={} samples={} bytes={} damaged_chunks={} damaged_samples={} \
         buffered={}",
        r.segment_count,
        r.chunk_count,
        r.sample_count,
        r.verified_bytes,
        r.damaged_chunk_count,
        r.damaged_sample_count,
        stats.buffered_sample_count
    )?;
    for d in &r.damaged_chunks {
        writeln!(
            out,
            "damaged {} offset={} length={} {}",
            d.segment, d.offset, d.length, d.description
        )?;
    }
    if r.damaged_chunk_count > 0 {
        let chunks = r.damaged_chunk_count;
        return Err(CliError::Refused(format!(
            "{name}: {chunks} damaged chunks"
        )));
    }
    if r.chunk_count == 0 && held > 0 {
        return Err(CliError::Refused(nothing_examined(
            &name,
            held,
            stats.buffered_sample_count,
        )));
    }
    Ok(())
}

/// Why a verify that read no chunk of a stream holding `held` samples, of
/// which `buffered` are in no segment yet, is a failure.
fn nothing_examined(name: &str, held: i64, buffered: i64) -> String {
    if buffered >= held {
        format!(
            "{name}: examined nothing: all {held} samples are still in the write buffers, \
             in no segment file yet; they are written within storage.max_chunk_age, so \
             verify again after that"
        )
    } else {
        format!(
            "{name}: examined nothing, although {} samples are stored; the daemon's \
             index does not list their chunks",
            held - buffered
        )
    }
}
