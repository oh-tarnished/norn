//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl benchmark`: a quick client-side append and query run against a
//! running daemon. It writes real samples, so it uses its own stream, and
//! refuses one that already holds any unless told otherwise.

mod append;
mod batch;
mod plan;
mod query;

use std::io::Write;

use norn_protocol::common::ValueType;
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, GetStreamRequest, Stream};
use tonic::Code;
use tonic::transport::Channel;

use self::append::append;
use self::batch::{batch_size, limits};
use self::plan::{Plan, plan};
use self::query::query;
use crate::CliError;
use crate::resources::call;

/// Creates what `plan` needs. Reads the stream first and creates nothing if it
/// is refused.
async fn prepare(channel: &Channel, plan: &Plan) -> Result<(), CliError> {
    let name = format!("streams/{}", plan.stream);
    let mut streams = StreamServiceClient::new(channel.clone());
    match streams
        .get_stream(GetStreamRequest { name: name.clone() })
        .await
    {
        Ok(found) => {
            let held = found.into_inner().stats.unwrap_or_default();
            let any = held.sample_count + held.corrupt_sample_count + held.stored_bytes;
            if any > 0 && !plan.allow_non_empty {
                return Err(CliError::Refused(format!(
                    "{name} already holds {} samples and a benchmark would add to them; \
                     pick an empty stream with --stream, or pass --allow-non-empty",
                    held.sample_count
                )));
            }
        }
        Err(status) if status.code() == Code::NotFound => {
            let request = CreateStreamRequest {
                stream_id: plan.stream.clone(),
                stream: Some(Stream::default()),
                validate_only: false,
            };
            let made = streams.create_stream(request).await;
            made.map_err(call("StreamService.CreateStream"))?;
        }
        Err(status) => return Err(call("StreamService.GetStream")(status)),
    }
    let request = CreateEntityRequest {
        entity_id: plan.stream.clone(),
        entity: Some(Entity::default()),
        validate_only: false,
    };
    let made = EntityServiceClient::new(channel.clone())
        .create_entity(request)
        .await;
    tolerate_existing(made, "EntityService.CreateEntity")?;
    let mut signals = SignalServiceClient::new(channel.clone());
    for n in 0..plan.signals {
        let request = CreateSignalRequest {
            parent: format!("entities/{}", plan.stream),
            signal_id: format!("s{n}"),
            signal: Some(Signal {
                value_type: ValueType::Double as i32,
                stream: name.clone(),
                ..Signal::default()
            }),
            validate_only: false,
        };
        let made = signals.create_signal(request).await;
        tolerate_existing(made, "SignalService.CreateSignal")?;
    }
    Ok(())
}

fn tolerate_existing<T>(
    made: Result<T, tonic::Status>,
    method: &'static str,
) -> Result<(), CliError> {
    match made {
        Err(status) if status.code() != Code::AlreadyExists => Err(call(method)(status)),
        _ => Ok(()),
    }
}

/// `nornctl benchmark`.
pub(crate) async fn benchmark(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let plan = plan(words)?;
    let limits = limits(&channel).await?;
    let batch = batch_size(&plan, &limits)?;
    prepare(&channel, &plan).await?;
    let rate = plan
        .rate
        .map_or_else(|| "max".to_owned(), |r| r.to_string());
    writeln!(out, "stream: streams/{}", plan.stream)?;
    writeln!(out, "signals: {}", plan.signals)?;
    writeln!(out, "target_rate: {rate}")?;
    writeln!(out, "duration: {:.3}s", plan.duration.as_secs_f64())?;
    writeln!(out, "batch_samples: {batch}")?;
    writeln!(out, "ack: {}", plan.ack.as_str_name())?;
    let appended = append(&channel, &plan, &limits, batch).await?;
    appended.print(out)?;
    query(&channel, &plan, appended.first, out).await
}
