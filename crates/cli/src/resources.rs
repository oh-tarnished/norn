//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `streams`, `entities`, `signals` and `append`, over the public API.

use std::io::Write;

use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity, ListEntitiesRequest};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, ListSignalsRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, ListStreamsRequest, Stream};
use tonic::transport::Channel;

use crate::CliError;
use crate::append::{append, value_type};
use crate::query::query;
use crate::replay::replay;
use crate::retention::retention;
use crate::state::state;
use crate::subscribe::subscribe;
use crate::transfer::{export, import};
use crate::upkeep::{compact, verify};

pub(crate) fn call(method: &'static str) -> impl FnOnce(tonic::Status) -> CliError {
    move |status| CliError::Call { method, status }
}

pub(crate) fn usage(text: &str) -> CliError {
    CliError::Usage(format!("usage: {text}"))
}

/// Runs a resource command; `words` starts with its name.
///
/// # Errors
/// [`CliError::Usage`]; [`CliError::Call`] with the daemon's reason;
/// [`CliError::Refused`] for a rejected batch; [`CliError::Output`].
pub async fn run(channel: Channel, words: &[String], out: &mut impl Write) -> Result<(), CliError> {
    let words: Vec<&str> = words.iter().map(String::as_str).collect();
    match words.as_slice() {
        ["state", rest @ ..] => state(channel, rest, out).await,
        ["replay", rest @ ..] => replay(channel, rest, out).await,
        ["subscribe", rest @ ..] => subscribe(channel, rest, out).await,
        ["export", rest @ ..] => export(channel, rest, out).await,
        ["import", rest @ ..] => import(channel, rest, out).await,
        ["compact", rest @ ..] => compact(channel, rest, out).await,
        ["verify", rest @ ..] => verify(channel, rest, out).await,
        ["retention", rest @ ..] => retention(channel, rest, out).await,
        ["streams"] => list_streams(channel, out).await,
        ["streams", "create", id] => {
            let request = CreateStreamRequest {
                stream_id: (*id).to_owned(),
                stream: Some(Stream::default()),
                validate_only: false,
            };
            let mut client = StreamServiceClient::new(channel);
            let made = client.create_stream(request).await;
            let made = made.map_err(call("StreamService.CreateStream"))?;
            Ok(writeln!(out, "{}", made.into_inner().name)?)
        }
        ["entities"] => list_entities(channel, out).await,
        ["entities", "create", id] => {
            let request = CreateEntityRequest {
                entity_id: (*id).to_owned(),
                entity: Some(Entity::default()),
                validate_only: false,
            };
            let mut client = EntityServiceClient::new(channel);
            let made = client.create_entity(request).await;
            let made = made.map_err(call("EntityService.CreateEntity"))?;
            Ok(writeln!(out, "{}", made.into_inner().name)?)
        }
        ["signals"] => list_signals(channel, "entities/-", out).await,
        ["signals", "create", entity, id, stream, kind] => {
            let value_type = value_type(kind)?;
            let request = CreateSignalRequest {
                parent: format!("entities/{entity}"),
                signal_id: (*id).to_owned(),
                signal: Some(Signal {
                    value_type: value_type as i32,
                    stream: format!("streams/{stream}"),
                    ..Signal::default()
                }),
                validate_only: false,
            };
            let mut client = SignalServiceClient::new(channel);
            let made = client.create_signal(request).await;
            let made = made.map_err(call("SignalService.CreateSignal"))?;
            Ok(writeln!(out, "{}", made.into_inner().name)?)
        }
        ["signals", entity] => list_signals(channel, &format!("entities/{entity}"), out).await,
        ["append", stream, signal, value, rest @ ..] => {
            append(channel, stream, signal, value, rest, out).await
        }
        ["query", rest @ ..] => query(channel, rest, out).await,
        _ => Err(usage("see nornctl --help")),
    }
}

async fn list_streams(channel: Channel, out: &mut impl Write) -> Result<(), CliError> {
    let mut client = StreamServiceClient::new(channel);
    let mut page_token = String::new();
    loop {
        let request = ListStreamsRequest {
            page_token,
            ..ListStreamsRequest::default()
        };
        let page = client.list_streams(request).await;
        let page = page
            .map_err(call("StreamService.ListStreams"))?
            .into_inner();
        for s in page.streams {
            let stats = s.stats.unwrap_or_default();
            write!(out, "{} samples={}", s.name, stats.sample_count)?;
            if stats.buffered_sample_count > 0 {
                write!(out, " buffered={}", stats.buffered_sample_count)?;
            }
            if stats.corrupt_sample_count > 0 {
                write!(out, " corrupt={}", stats.corrupt_sample_count)?;
            }
            writeln!(out)?;
        }
        page_token = page.next_page_token;
        if page_token.is_empty() {
            return Ok(());
        }
    }
}

async fn list_entities(channel: Channel, out: &mut impl Write) -> Result<(), CliError> {
    let mut client = EntityServiceClient::new(channel);
    let mut page_token = String::new();
    loop {
        let request = ListEntitiesRequest {
            page_token,
            ..ListEntitiesRequest::default()
        };
        let page = client.list_entities(request).await;
        let page = page
            .map_err(call("EntityService.ListEntities"))?
            .into_inner();
        for e in page.entities {
            writeln!(out, "{}", e.name)?;
        }
        page_token = page.next_page_token;
        if page_token.is_empty() {
            return Ok(());
        }
    }
}

async fn list_signals(
    channel: Channel,
    parent: &str,
    out: &mut impl Write,
) -> Result<(), CliError> {
    let mut client = SignalServiceClient::new(channel);
    let mut page_token = String::new();
    loop {
        let request = ListSignalsRequest {
            parent: parent.to_owned(),
            page_token,
            ..ListSignalsRequest::default()
        };
        let page = client.list_signals(request).await;
        let page = page
            .map_err(call("SignalService.ListSignals"))?
            .into_inner();
        for s in page.signals {
            let kind = ValueType::try_from(s.value_type).map_or("?", |t| t.as_str_name());
            writeln!(out, "{} {kind} {}", s.name, s.stream)?;
        }
        page_token = page.next_page_token;
        if page_token.is_empty() {
            return Ok(());
        }
    }
}
