//! The history the unary reads are tested on, seeded in a real daemon and kept as
//! plain data, which the tests answer from by brute force. Ported from the MCP
//! server's tests, whose tools these RPCs replace.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, Sample, Value, ValueType};
use norn_protocol::entity::entity_service_client::EntityServiceClient;
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::signal::signal_service_client::SignalServiceClient;
use norn_protocol::signal::{CreateSignalRequest, Signal};
use norn_protocol::stream::stream_service_client::StreamServiceClient;
use norn_protocol::stream::{CreateStreamRequest, Stream};
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{AcknowledgementLevel, AppendSamplesRequest};
use tonic::transport::Channel;

pub const TEMP: &str = "entities/ahu/signals/temp";
pub const ON: &str = "entities/ahu/signals/on";
pub const MODE: &str = "entities/ahu/signals/mode";
pub const FLOW: &str = "entities/pump/signals/flow";

/// One seeded sample.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub signal: &'static str,
    pub second: i64,
    pub value: Kind,
    pub bad: bool,
    pub sequence: i64,
}

fn row(signal: &'static str, second: i64, value: Kind, n: i64) -> Row {
    Row {
        signal,
        second,
        value,
        bad: false,
        sequence: n,
    }
}

/// Every seeded sample, in (signal name, time) order: the storage order. `mode`
/// holds one 5,000-byte string, so a value is cut; `temp` turns BAD every 97th.
pub fn history() -> Vec<Row> {
    let mut rows: Vec<Row> = (0..200)
        .map(|n| row(ON, 1_000 + n, Kind::BoolValue((n / 10) % 2 == 0), n))
        .collect();
    rows.extend((0..100).map(|n| {
        let text = if n == 50 {
            "x".repeat(5_000)
        } else {
            format!("m{}", n / 20)
        };
        row(MODE, 1_000 + n, Kind::StringValue(text), n)
    }));
    rows.extend((0..500).map(|n| Row {
        bad: n % 97 == 0,
        ..row(TEMP, 1_000 + n, Kind::DoubleValue((n % 50) as f64 * 0.5), n)
    }));
    rows.extend((0..300).map(|n| row(FLOW, 1_100 + n, Kind::Int64Value(n * 3), n)));
    rows.sort_by_key(|r| (r.signal, r.second));
    rows
}

/// The history of `signal`, with `keep`.
pub fn of(signal: &str, keep: impl Fn(&Row) -> bool) -> Vec<Row> {
    history()
        .into_iter()
        .filter(|r| r.signal == signal && keep(r))
        .collect()
}

/// `r`'s value as a number, for the numeric signals.
pub fn number(r: &Row) -> f64 {
    match r.value {
        Kind::DoubleValue(v) => v,
        Kind::Int64Value(v) => v as f64,
        _ => panic!("not a number"),
    }
}

fn wire(r: &Row) -> Sample {
    let severity = if r.bad {
        QualitySeverity::Bad
    } else {
        QualitySeverity::Good
    };
    Sample {
        signal: r.signal.to_owned(),
        observe_time: at(r.second),
        value: Some(Value {
            kind: Some(r.value.clone()),
        }),
        quality: Some(Quality {
            severity: severity as i32,
            substatus: 0,
        }),
        sequence: r.sequence,
    }
}

/// `second` past the epoch.
pub fn at(second: i64) -> Option<prost_types::Timestamp> {
    Some(prost_types::Timestamp {
        seconds: second,
        nanos: 0,
    })
}

/// The seconds of a sample's time.
pub fn second(s: &Sample) -> i64 {
    s.observe_time.unwrap().seconds
}

pub fn query(c: &Channel) -> QueryServiceClient<Channel> {
    QueryServiceClient::new(c.clone())
}

async fn signal(c: &Channel, entity: &str, id: &str, kind: ValueType) {
    let request = CreateSignalRequest {
        parent: format!("entities/{entity}"),
        signal_id: id.to_owned(),
        signal: Some(Signal {
            value_type: kind as i32,
            stream: "streams/plant".to_owned(),
            ..Signal::default()
        }),
        validate_only: false,
    };
    SignalServiceClient::new(c.clone())
        .create_signal(request)
        .await
        .unwrap();
}

/// Creates the catalogue and appends [`history`], durably, on `c`.
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
            entity: Some(Entity::default()),
            validate_only: false,
        };
        EntityServiceClient::new(c.clone())
            .create_entity(request)
            .await
            .unwrap();
    }
    signal(c, "ahu", "temp", ValueType::Double).await;
    signal(c, "ahu", "on", ValueType::Bool).await;
    signal(c, "ahu", "mode", ValueType::String).await;
    signal(c, "pump", "flow", ValueType::Int64).await;
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
