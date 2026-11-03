//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl retention`: read a stream's policy, set it, apply it.

use std::io::Write;

use norn_protocol::stream::retention_policy::Limit;
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{GetStreamRequest, RetentionPolicy, Stream, UpdateStreamRequest};
use tonic::transport::Channel;

use crate::CliError;
use crate::query::window;
use crate::resources::{call, usage};
use crate::upkeep::{apply, stream_name};

const USAGE: &str =
    "retention get STREAM | set STREAM forever|Nd|Nmo|Nb | apply STREAM [--dry-run]";

/// A policy from `forever`, `30d` (any duration), `6mo` or `5000000b`.
fn policy(word: &str) -> Result<Option<RetentionPolicy>, CliError> {
    if word == "forever" {
        return Ok(None);
    }
    let limit = if let Some(n) = word.strip_suffix("mo") {
        Limit::MaxMonths(n.parse().map_err(|_| usage(USAGE))?)
    } else if let Some(n) = word.strip_suffix('b') {
        Limit::MaxBytes(n.parse().map_err(|_| usage(USAGE))?)
    } else {
        Limit::MaxAge(window(word)?)
    };
    Ok(Some(RetentionPolicy { limit: Some(limit) }))
}

fn show(policy: Option<&RetentionPolicy>) -> String {
    match policy.and_then(|p| p.limit.as_ref()) {
        Some(Limit::MaxAge(age)) => format!("max_age={}s", age.seconds),
        Some(Limit::MaxBytes(n)) => format!("max_bytes={n}"),
        Some(Limit::MaxMonths(n)) => format!("max_months={n}"),
        None => "forever".to_owned(),
    }
}

/// Runs a `retention` subcommand.
pub(crate) async fn retention(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let mut client = StreamServiceClient::new(channel.clone());
    match words {
        ["get", stream] => {
            let name = stream_name(stream);
            let got = client
                .get_stream(GetStreamRequest { name: name.clone() })
                .await;
            let got = got.map_err(call("StreamService.GetStream"))?.into_inner();
            Ok(writeln!(out, "{name} {}", show(got.retention.as_ref()))?)
        }
        ["set", stream, word] => {
            let name = stream_name(stream);
            let request = UpdateStreamRequest {
                stream: Some(Stream {
                    name: name.clone(),
                    retention: policy(word)?,
                    ..Stream::default()
                }),
                update_mask: Some(prost_types::FieldMask {
                    paths: vec!["retention".to_owned()],
                }),
                validate_only: false,
            };
            let set = client.update_stream(request).await;
            let set = set
                .map_err(call("StreamService.UpdateStream"))?
                .into_inner();
            Ok(writeln!(out, "{name} {}", show(set.retention.as_ref()))?)
        }
        ["apply", stream] => apply(channel, stream_name(stream), false, out).await,
        ["apply", stream, "--dry-run"] => apply(channel, stream_name(stream), true, out).await,
        _ => Err(usage(USAGE)),
    }
}
