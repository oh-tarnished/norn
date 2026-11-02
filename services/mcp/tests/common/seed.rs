//! The seeded history, and the same history as plain data, which tests answer from
//! by brute force.

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest};
use serde_json::{Value as Json, json};
use tonic::transport::Channel;

pub const TEMP: &str = "entities/ahu/signals/temp";
pub const ON: &str = "entities/ahu/signals/on";
pub const MODE: &str = "entities/ahu/signals/mode";
pub const FLOW: &str = "entities/pump/signals/flow";

/// One seeded sample, as a result shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub signal: &'static str,
    pub second: i64,
    pub value: Json,
    pub bad: bool,
    pub sequence: i64,
}

/// `second` past the epoch, as results print it; the seeds are in the first hour.
pub fn time(second: i64) -> String {
    format!("1970-01-01T00:{:02}:{:02}Z", second / 60, second % 60)
}

fn temp(n: i64) -> Row {
    Row {
        signal: TEMP,
        second: 1_000 + n,
        value: json!((n % 50) as f64 * 0.5),
        bad: n % 97 == 0,
        sequence: n,
    }
}

fn on(n: i64) -> Row {
    Row {
        signal: ON,
        second: 1_000 + n,
        value: json!((n / 10) % 2 == 0),
        bad: false,
        sequence: n,
    }
}

fn mode(n: i64) -> Row {
    let text = if n == 50 {
        "x".repeat(5_000)
    } else {
        format!("m{}", n / 20)
    };
    Row {
        signal: MODE,
        second: 1_000 + n,
        value: json!(text),
        bad: false,
        sequence: n,
    }
}

fn flow(n: i64) -> Row {
    Row {
        signal: FLOW,
        second: 1_100 + n,
        value: json!(n * 3),
        bad: false,
        sequence: n,
    }
}

/// Every seeded sample, in (signal's full name, time) order.
pub fn history() -> Vec<Row> {
    let mut rows: Vec<Row> = (0..200).map(on).collect();
    rows.extend((0..100).map(mode));
    rows.extend((0..500).map(temp));
    rows.extend((0..300).map(flow));
    rows.sort_by_key(|r| (r.signal, r.second));
    rows
}

fn wire(r: &Row) -> Sample {
    let kind = match r.signal {
        TEMP => Kind::DoubleValue(r.value.as_f64().unwrap()),
        ON => Kind::BoolValue(r.value.as_bool().unwrap()),
        MODE => Kind::StringValue(r.value.as_str().unwrap().to_owned()),
        _ => Kind::Int64Value(r.value.as_i64().unwrap()),
    };
    let severity = if r.bad {
        QualitySeverity::Bad
    } else {
        QualitySeverity::Good
    };
    Sample {
        signal: r.signal.to_owned(),
        observe_time: Some(prost_types::Timestamp {
            seconds: r.second,
            nanos: 0,
        }),
        value: Some(Value { kind: Some(kind) }),
        quality: Some(Quality {
            severity: severity as i32,
            substatus: 0,
        }),
        sequence: r.sequence,
    }
}

async fn signal(c: &Channel, entity: &str, id: &str, kind: ValueType, unit: &str) {
    let request = CreateSignalRequest {
        parent: format!("entities/{entity}"),
        signal_id: id.to_owned(),
        signal: Some(Signal {
            value_type: kind as i32,
            stream: "streams/plant".to_owned(),
            unit: unit.to_owned(),
            ..Signal::default()
        }),
        validate_only: false,
    };
    SignalServiceClient::new(c.clone())
        .create_signal(request)
        .await
        .unwrap();
}

/// Creates the catalogue and appends [`history`], durably, on `channel`.
pub async fn seed(c: &Channel) {
    let stream = CreateStreamRequest {
        stream_id: "plant".to_owned(),
        stream: Some(Stream::default()),
        validate_only: false,
    };
    StreamServiceClient::new(c.clone())
        .create_stream(stream)
        .await
        .unwrap();
    for id in ["ahu", "pump"] {
        let request = CreateEntityRequest {
            entity_id: id.to_owned(),
            entity: Some(Entity {
                display_name: id.to_uppercase(),
                ..Entity::default()
            }),
            validate_only: false,
        };
        EntityServiceClient::new(c.clone())
            .create_entity(request)
            .await
            .unwrap();
    }
    signal(c, "ahu", "temp", ValueType::Double, "Cel").await;
    signal(c, "ahu", "on", ValueType::Bool, "").await;
    signal(c, "ahu", "mode", ValueType::String, "").await;
    signal(c, "pump", "flow", ValueType::Int64, "L/min").await;
    let batch = AppendSamplesRequest {
        stream: "streams/plant".to_owned(),
        batch_id: 1,
        samples: history().iter().map(wire).collect(),
        ack_level: AcknowledgementLevel::Durable as i32,
    };
    let requests = tonic::codegen::tokio_stream::iter([batch]);
    let mut client = TelemetryServiceClient::new(c.clone());
    let mut replies = client.append_samples(requests).await.unwrap().into_inner();
    while let Some(reply) = replies.message().await.unwrap() {
        assert!(reply.error.is_none(), "{reply:?}");
    }
}

/// The history of `signal`, with `keep`.
pub fn of(signal: &str, keep: impl Fn(&Row) -> bool) -> Vec<Row> {
    history()
        .into_iter()
        .filter(|r| r.signal == signal && keep(r))
        .collect()
}
