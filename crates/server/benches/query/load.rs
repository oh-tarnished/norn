//! Loading the data set through `AppendSamples`, honouring backpressure.

use std::sync::Arc;
use std::time::Duration;

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{
    AcknowledgementLevel, AppendSamplesRequest, AppendSamplesResponse, BackpressureOutcome,
};
use tokio::sync::mpsc;
use tonic::transport::Channel;

use super::driver::Requests;

const BATCH: i64 = 5_000;

fn sample(name: &str, signal: usize, time: i64, second: i64) -> Sample {
    // A drift with 0.01 of deterministic noise, as a real sensor's decimals look:
    // about one byte a sample stored, where a clean sawtooth takes almost nothing.
    let mix = (second as u64 ^ (signal as u64) << 32).wrapping_mul(0x9e37_79b9_7f4a_7c15);
    let noise = (mix >> 54) as f64 / 100.0;
    let wave = ((second + signal as i64 * 97) % 3_600) as f64 / 10.0 + noise;
    Sample {
        signal: name.to_owned(),
        observe_time: Some(prost_types::Timestamp {
            seconds: time,
            nanos: 0,
        }),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(20.0 + wave)),
        }),
        quality: Some(Quality {
            severity: QualitySeverity::Good as i32,
            substatus: 0,
        }),
        sequence: second,
    }
}

/// One sample a second per signal for `seconds`, in time order, one batch awaited
/// at a time so a slow disk slows the load and nothing is refused.
pub async fn load(channel: &Channel, names: &Arc<Vec<String>>, start: i64, seconds: i64) {
    let (tx, rx) = mpsc::channel::<AppendSamplesRequest>(1);
    let mut client = TelemetryServiceClient::new(channel.clone());
    let replies = client.append_samples(Requests(rx)).await.unwrap();
    let mut replies = replies.into_inner();
    let mut samples = Vec::new();
    let mut id = 0;
    for second in 0..seconds {
        for (signal, name) in names.iter().enumerate() {
            samples.push(sample(name, signal, start + second, second));
            let last = second + 1 == seconds && signal + 1 == names.len();
            if samples.len() as i64 >= BATCH || last {
                id += 1;
                send(&tx, &mut replies, &mut id, &samples).await;
                samples.clear();
            }
        }
    }
}

/// Sends one batch, again and again until it is admitted.
async fn send(
    tx: &mpsc::Sender<AppendSamplesRequest>,
    replies: &mut tonic::Streaming<AppendSamplesResponse>,
    id: &mut i64,
    samples: &[Sample],
) {
    loop {
        let batch = AppendSamplesRequest {
            stream: "streams/plant".into(),
            batch_id: *id,
            samples: samples.to_vec(),
            ack_level: AcknowledgementLevel::Persisted as i32,
        };
        tx.send(batch).await.unwrap();
        let first = replies.message().await.unwrap().unwrap();
        let admission = first.admission.unwrap();
        if admission.outcome != BackpressureOutcome::Reject as i32 {
            if first.ack_level != AcknowledgementLevel::Persisted as i32 {
                replies.message().await.unwrap().unwrap();
            }
            if admission.outcome == BackpressureOutcome::Throttle as i32 {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            return;
        }
        assert!(admission.retry_delay.is_some(), "{admission:?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
        *id += 1;
    }
}
