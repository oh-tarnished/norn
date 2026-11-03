//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `nornctl append`: one sample through the real `AppendSamples` call.

use std::io::Write;

use norn_ipc::{AppendCall, DataPlaneKind};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{EnumValue, Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest, BackpressureOutcome};
use tonic::transport::Channel;

use crate::CliError;
use crate::resources::{call, usage};

pub(crate) fn value_type(word: &str) -> Result<ValueType, CliError> {
    Ok(match word {
        "bool" => ValueType::Bool,
        "int32" => ValueType::Int32,
        "int64" => ValueType::Int64,
        "uint32" => ValueType::Uint32,
        "uint64" => ValueType::Uint64,
        "float" => ValueType::Float,
        "double" => ValueType::Double,
        "string" => ValueType::String,
        "bytes" => ValueType::Bytes,
        "enum" => ValueType::Enum,
        _ => return Err(usage("TYPE is one of bool, int32, ... (see --help)")),
    })
}

/// Parses `text` as the type the signal declares; its type is read from the
/// daemon, so the command line needs no type of its own.
fn parse_value(text: &str, kind: ValueType) -> Result<Value, CliError> {
    let bad = |_: std::num::ParseIntError| usage("VALUE does not fit the signal's type");
    let kind = match kind {
        ValueType::Bool => {
            Kind::BoolValue(text.parse().map_err(|_| usage("VALUE: true or false"))?)
        }
        ValueType::Int32 => Kind::Int32Value(text.parse().map_err(bad)?),
        ValueType::Int64 => Kind::Int64Value(text.parse().map_err(bad)?),
        ValueType::Uint32 => Kind::Uint32Value(text.parse().map_err(bad)?),
        ValueType::Uint64 => {
            let v: u64 = text.parse().map_err(bad)?;
            Kind::Uint64Value(v as i64)
        }
        ValueType::Float => Kind::FloatValue(text.parse().map_err(|_| usage("VALUE: a number"))?),
        ValueType::Double => Kind::DoubleValue(text.parse().map_err(|_| usage("VALUE: a number"))?),
        ValueType::String => Kind::StringValue(text.to_owned()),
        ValueType::Bytes => Kind::BytesValue(text.as_bytes().to_vec()),
        ValueType::Enum => Kind::EnumValue(EnumValue {
            number: text.parse().map_err(bad)?,
            symbol: String::new(),
        }),
        ValueType::Unspecified => return Err(usage("the signal has no type")),
    };
    Ok(Value { kind: Some(kind) })
}

pub(crate) async fn append(
    channel: Channel,
    stream: &str,
    signal: &str,
    value: &str,
    flags: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let (mut level, mut sequence) = (AcknowledgementLevel::Durable, 0i64);
    let mut at = None;
    let (options, flags) = crate::plane::options(flags);
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = flags.next().ok_or_else(|| usage("a flag needs a value"))?;
        match *flag {
            "--ack" => {
                level = match *word {
                    "buffered" => AcknowledgementLevel::Buffered,
                    "persisted" => AcknowledgementLevel::Persisted,
                    "durable" => AcknowledgementLevel::Durable,
                    _ => return Err(usage("LEVEL is buffered, persisted or durable")),
                };
            }
            "--sequence" => {
                sequence = word.parse().map_err(|_| usage("--sequence: a number"))?;
            }
            "--at" => at = Some(crate::query::timestamp(word)?),
            _ => {
                return Err(usage(
                    "append takes --ack, --sequence, --at, --shared-memory",
                ));
            }
        }
    }
    let kind = {
        let mut client = SignalServiceClient::new(channel.clone());
        let request = norn_protocol::signal::GetSignalRequest {
            name: signal.to_owned(),
        };
        let found = client.get_signal(request).await;
        found
            .map_err(call("SignalService.GetSignal"))?
            .into_inner()
            .value_type
    };
    let kind = ValueType::try_from(kind).unwrap_or(ValueType::Unspecified);
    let observe_time = match at {
        Some(at) => at,
        None => crate::query::timestamp("now")?,
    };
    let sample = Sample {
        signal: signal.to_owned(),
        observe_time: Some(observe_time),
        value: Some(parse_value(value, kind)?),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence,
    };
    let batch = AppendSamplesRequest {
        stream: format!("streams/{stream}"),
        batch_id: 1,
        samples: vec![sample],
        ack_level: level as i32,
    };
    let method = "TelemetryService.AppendSamples";
    let opened = AppendCall::open(channel, &options)
        .await
        .map_err(call(method))?;
    let plane = opened.plane();
    let sent = opened.send(batch).await;
    // Closing the request side: the daemon finishes the batch and ends the call.
    let (sender, mut replies) = opened.into_parts();
    drop(sender);
    if let Err(e) = sent {
        replies.message().await.map_err(call(method))?;
        return Err(CliError::Refused(format!(
            "the call ended before the batch: {e}"
        )));
    }
    if plane == DataPlaneKind::SharedMemory {
        writeln!(out, "plane SHARED_MEMORY")?;
    }
    while let Some(reply) = replies.message().await.map_err(call(method))? {
        let admission = reply.admission.unwrap_or_default();
        let outcome = BackpressureOutcome::try_from(admission.outcome);
        let outcome = outcome.unwrap_or(BackpressureOutcome::Unspecified);
        if outcome == BackpressureOutcome::Reject || reply.error.is_some() {
            let text = reply.error.map(|e| e.message).unwrap_or(admission.message);
            let errors = reply.sample_errors.into_iter();
            let detail: Vec<String> = errors.filter_map(|e| e.status.map(|s| s.message)).collect();
            return Err(CliError::Refused(format!(
                "{}: {text} {}",
                outcome.as_str_name(),
                detail.join("; ")
            )));
        }
        let reached = AcknowledgementLevel::try_from(reply.ack_level);
        let reached = reached.unwrap_or(AcknowledgementLevel::Unspecified);
        writeln!(out, "{} {}", outcome.as_str_name(), reached.as_str_name())?;
    }
    Ok(())
}
