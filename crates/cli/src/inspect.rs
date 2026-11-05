//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl inspect`: how a stream's history sits on disk, read-only, through
//! `StreamService.ListSegments` and `GetSegment`.

use std::io::Write;

use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{
    ChunkColumn, GetSegmentRequest, GetStreamRequest, ListSegmentsRequest, Segment, SegmentView,
};
use prost_types::Timestamp;
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};
use crate::time::format;
use crate::upkeep::stream_name;

const USAGE: &str = "inspect STREAM [SEGMENT] [--basic]";

fn time(t: Option<Timestamp>) -> String {
    t.map_or_else(|| "-".to_owned(), |t| format(t.seconds, t.nanos))
}

/// `decoded / stored` to two places, or `-` when nothing was measured.
fn ratio(decoded: i64, stored: i64) -> String {
    if decoded <= 0 || stored <= 0 {
        return "-".to_owned();
    }
    format!("{:.2}", decoded as f64 / stored as f64)
}

fn print(out: &mut impl Write, s: &Segment, full: bool) -> std::io::Result<()> {
    write!(
        out,
        "{} sealed={} generation={} chunks={} samples={} bytes={} from={} to={}",
        s.name,
        s.sealed,
        s.generation,
        s.chunk_count,
        s.sample_count,
        s.size_bytes,
        time(s.oldest_sample_time),
        time(s.newest_sample_time),
    )?;
    if !full {
        return writeln!(out);
    }
    writeln!(
        out,
        " signals={} decoded_bytes={} ratio={} damaged_chunks={}",
        s.signal_count,
        s.decoded_bytes,
        ratio(s.decoded_bytes, s.size_bytes),
        s.damaged_chunk_count,
    )?;
    for e in &s.column_encodings {
        let column = ChunkColumn::try_from(e.column).unwrap_or(ChunkColumn::Unspecified);
        writeln!(
            out,
            "  {} codec={} zstd={} chunks={} stored_bytes={} encoded_bytes={}",
            column.as_str_name().trim_start_matches("CHUNK_COLUMN_"),
            e.codec,
            e.zstd_level,
            e.chunk_count,
            e.stored_bytes,
            e.encoded_bytes,
        )?;
    }
    Ok(())
}

/// `nornctl inspect STREAM [SEGMENT] [--basic]`. Reads every chunk of what it
/// names, for the codecs and the ratio, unless `--basic` asks only for what the
/// index holds.
pub(crate) async fn inspect(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let full = !words.contains(&"--basic");
    let names: Vec<&str> = words.iter().copied().filter(|w| *w != "--basic").collect();
    if names.iter().any(|n| n.starts_with('-')) {
        return Err(usage(USAGE));
    }
    let view = if full {
        SegmentView::Full
    } else {
        SegmentView::Basic
    } as i32;
    let mut client = StreamServiceClient::new(channel);
    match names.as_slice() {
        [stream, segment] => {
            let name = format!("{}/segments/{segment}", stream_name(stream));
            let found = client.get_segment(GetSegmentRequest { name, view }).await;
            let found = found.map_err(call("StreamService.GetSegment"))?;
            Ok(print(out, &found.into_inner(), full)?)
        }
        [stream] => whole(&mut client, &stream_name(stream), view, full, out).await,
        _ => Err(usage(USAGE)),
    }
}

async fn whole(
    client: &mut StreamServiceClient<Channel>,
    parent: &str,
    view: i32,
    full: bool,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let name = parent.to_owned();
    let stream = client.get_stream(GetStreamRequest { name }).await;
    let stream = stream.map_err(call("StreamService.GetStream"))?;
    let stats = stream.into_inner().stats.unwrap_or_default();
    writeln!(
        out,
        "{parent} samples={} buffered={} corrupt={} stored_bytes={}",
        stats.sample_count,
        stats.buffered_sample_count,
        stats.corrupt_sample_count,
        stats.stored_bytes,
    )?;
    let (mut segments, mut chunks, mut samples, mut bytes, mut decoded) = (0i64, 0, 0, 0, 0);
    let mut page_token = String::new();
    loop {
        let request = ListSegmentsRequest {
            parent: parent.to_owned(),
            page_token,
            view,
            ..ListSegmentsRequest::default()
        };
        let page = client.list_segments(request).await;
        let page = page
            .map_err(call("StreamService.ListSegments"))?
            .into_inner();
        for s in &page.segments {
            print(out, s, full)?;
            segments += 1;
            chunks += s.chunk_count;
            samples += s.sample_count;
            bytes += s.size_bytes;
            decoded += s.decoded_bytes;
        }
        page_token = page.next_page_token;
        if page_token.is_empty() {
            break;
        }
    }
    write!(
        out,
        "total segments={segments} chunks={chunks} samples={samples} bytes={bytes}"
    )?;
    if full {
        write!(out, " ratio={}", ratio(decoded, bytes))?;
    }
    Ok(writeln!(out)?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_ratio_needs_both_sizes() {
        assert_eq!(ratio(300, 100), "3.00");
        assert_eq!(ratio(0, 100), "-");
        assert_eq!(ratio(300, 0), "-");
    }
}
