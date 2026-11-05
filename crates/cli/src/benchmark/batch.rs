//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What `nornctl benchmark` sends: samples, in batches sized from the daemon's
//! published limits.

use std::time::{SystemTime, UNIX_EPOCH};

use norn_protocol::admin::admin_service_client::AdminServiceClient;
use norn_protocol::admin::{GetServerRequest, Limits};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use prost::Message;
use prost_types::Timestamp;
use tonic::transport::Channel;

use super::plan::Plan;
use crate::CliError;
use crate::resources::{call, usage};

/// How often batches go out at most, when the rate leaves the choice open.
const TICK_PER_SECOND: u64 = 100;
/// The default batch at `--rate max`.
const MAX_RATE_BATCH: usize = 1000;

pub(super) async fn limits(channel: &Channel) -> Result<Limits, CliError> {
    let name = "server".to_owned();
    let server = AdminServiceClient::new(channel.clone())
        .get_server(GetServerRequest { name })
        .await
        .map_err(call("AdminService.GetServer"))?;
    server.into_inner().limits.ok_or(CliError::Malformed {
        method: "AdminService.GetServer",
        field: "limits",
    })
}

pub(super) fn sample(plan: &Plan, n: u64, nanos: i128) -> Sample {
    let signal = n % plan.signals as u64;
    Sample {
        signal: format!("entities/{}/signals/s{signal}", plan.stream),
        observe_time: Some(Timestamp {
            seconds: (nanos / 1_000_000_000) as i64,
            nanos: (nanos % 1_000_000_000) as i32,
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(n as f64)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: i64::try_from(n).unwrap_or(i64::MAX),
    }
}

pub(super) fn now_nanos() -> i128 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH);
    now.unwrap_or_default().as_nanos() as i128
}

/// Samples per batch: `--batch`, or a tick's worth, within what the daemon publishes.
pub(super) fn batch_size(plan: &Plan, limits: &Limits) -> Result<usize, CliError> {
    let one = sample(plan, u64::MAX >> 1, i128::from(i64::MAX)).encoded_len() + 8;
    let bytes = usize::try_from(limits.max_batch_bytes).unwrap_or(0) / one;
    let most = bytes.min(usize::try_from(limits.max_batch_samples).unwrap_or(0));
    match plan.batch {
        Some(n) if n > most => Err(usage(&format!(
            "--batch: this daemon's limits allow at most {most} samples a batch"
        ))),
        Some(n) => Ok(n),
        None => {
            let wanted = plan
                .rate
                .map_or(MAX_RATE_BATCH as u64, |r| r / TICK_PER_SECOND);
            Ok(usize::try_from(wanted)
                .unwrap_or(most)
                .clamp(1, most.max(1)))
        }
    }
}
