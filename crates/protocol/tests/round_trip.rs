//! Encode then decode reproduces every message exactly (SPEC section 36).

use norn_protocol::common::{
    EnumValue, Quality, QualitySeverity, QualitySubstatus, Sample, Value, value::Kind,
};
use norn_protocol::rpc::pb::{ErrorInfo, Status};
use norn_protocol::telemetry::{
    AcknowledgementLevel, Admission, AppendSamplesRequest, AppendSamplesResponse,
    BackpressureOutcome, BackpressureReason, ClockEvent, ClockEventKind, Event, SampleError,
    SubscribeEventsResponse, SubscriptionMode, event::Payload,
};
use prost::{DecodeError, Message};
use prost_types::{Any, Duration, Timestamp};

const ERROR_INFO_URL: &str = "type.googleapis.com/google.rpc.ErrorInfo";

/// Decodes `message`'s encoding and checks nothing changed. Bytes are the
/// authority: `PartialEq` would call a NaN unequal to itself, and the bytes
/// also prove a NaN's payload survived.
fn round_trip<M: Message + Default + std::fmt::Debug>(message: &M) -> Result<(), DecodeError> {
    let bytes = message.encode_to_vec();
    let decoded = M::decode(bytes.as_slice())?;
    assert_eq!(
        decoded.encode_to_vec(),
        bytes,
        "re-encoding changed the bytes"
    );
    assert_eq!(format!("{decoded:?}"), format!("{message:?}"));
    Ok(())
}

fn sample(kind: Kind, nanos: i32, sequence: i64) -> Sample {
    Sample {
        signal: "entities/unit-7/signals/outlet-temperature".into(),
        observe_time: Some(Timestamp {
            seconds: 1_798_761_600,
            nanos,
        }),
        value: Some(Value { kind: Some(kind) }),
        quality: Some(Quality {
            severity: QualitySeverity::Uncertain.into(),
            substatus: QualitySubstatus::Stale.into(),
        }),
        sequence,
    }
}

/// One sample per value type, at the edges of each type's range.
fn every_value_kind() -> Vec<Sample> {
    let kinds = [
        Kind::BoolValue(true),
        Kind::Int32Value(i32::MIN),
        Kind::Int64Value(i64::MAX),
        Kind::Uint32Value(i64::from(u32::MAX)),
        Kind::Uint64Value(u64::MAX.cast_signed()),
        Kind::FloatValue(f32::from_bits(0x7fc0_0001)),
        Kind::DoubleValue(-0.0),
        Kind::StringValue("μ-ändern".into()),
        Kind::BytesValue(vec![0, 255, 7]),
        Kind::EnumValue(EnumValue {
            number: i32::MAX,
            symbol: "OCCUPIED".into(),
        }),
    ];
    kinds
        .into_iter()
        .zip(0..)
        .map(|(kind, i)| sample(kind, 999_999_999 - i, i64::MAX - i64::from(i)))
        .collect()
}

#[test]
fn an_ingestion_batch_survives_encode_and_decode() {
    let batch = AppendSamplesRequest {
        stream: "streams/plant".into(),
        batch_id: 42,
        samples: every_value_kind(),
        ack_level: AcknowledgementLevel::Durable.into(),
    };
    round_trip(&batch).unwrap();
}

#[test]
fn a_nan_payload_keeps_its_exact_bits() {
    let batch = AppendSamplesRequest {
        samples: every_value_kind(),
        ..Default::default()
    };
    let decoded = AppendSamplesRequest::decode(batch.encode_to_vec().as_slice()).unwrap();
    let float = decoded
        .samples
        .iter()
        .find_map(|s| match s.value.as_ref()?.kind {
            Some(Kind::FloatValue(f)) => Some(f),
            _ => None,
        })
        .unwrap();
    assert_eq!(float.to_bits(), 0x7fc0_0001);
}

#[test]
fn a_rejection_with_its_reason_and_sample_errors_survives_encode_and_decode() {
    let info = ErrorInfo {
        reason: "ERROR_REASON_SIGNAL_NOT_FOUND".into(),
        domain: "norn.local".into(),
        metadata: [("signal".to_owned(), "entities/x/signals/y".to_owned())].into(),
    };
    let status = Status {
        code: 5,
        message: "no such signal".into(),
        details: vec![Any {
            type_url: ERROR_INFO_URL.into(),
            value: info.encode_to_vec(),
        }],
    };
    let response = AppendSamplesResponse {
        batch_id: 42,
        admission: Some(Admission {
            outcome: BackpressureOutcome::Reject.into(),
            reason: BackpressureReason::InvalidSamples.into(),
            retry_delay: None,
            message: "1 invalid sample".into(),
        }),
        ack_level: AcknowledgementLevel::Unspecified.into(),
        error: Some(status.clone()),
        sample_errors: vec![SampleError {
            index: 3,
            status: Some(status),
        }],
        duplicate_count: 0,
    };
    round_trip(&response).unwrap();
    let detail = &response.sample_errors[0].status.as_ref().unwrap().details[0];
    assert_eq!(detail.type_url, ERROR_INFO_URL);
    assert_eq!(ErrorInfo::decode(detail.value.as_slice()).unwrap(), info);
}

#[test]
fn a_throttle_carries_its_retry_delay() {
    let response = AppendSamplesResponse {
        batch_id: 7,
        admission: Some(Admission {
            outcome: BackpressureOutcome::Throttle.into(),
            reason: BackpressureReason::BufferHighWater.into(),
            retry_delay: Some(Duration {
                seconds: 0,
                nanos: 250_000_000,
            }),
            message: String::new(),
        }),
        ack_level: AcknowledgementLevel::Buffered.into(),
        ..Default::default()
    };
    round_trip(&response).unwrap();
}

#[test]
fn live_and_replayed_events_survive_encode_and_decode_as_one_message() {
    let clock = Timestamp {
        seconds: 1_798_761_601,
        nanos: 5,
    };
    let events = SubscribeEventsResponse {
        events: vec![
            Event {
                mode: SubscriptionMode::Live.into(),
                event_sequence: 1,
                clock_time: Some(clock),
                payload: Some(Payload::Sample(every_value_kind().remove(4))),
            },
            Event {
                mode: SubscriptionMode::Replay.into(),
                event_sequence: 2,
                clock_time: Some(clock),
                payload: Some(Payload::ClockEvent(ClockEvent {
                    kind: ClockEventKind::Seeked.into(),
                })),
            },
        ],
    };
    round_trip(&events).unwrap();
}

#[test]
fn a_truncated_batch_is_a_decode_error_not_a_panic() {
    let bytes = AppendSamplesRequest {
        samples: every_value_kind(),
        ..Default::default()
    }
    .encode_to_vec();
    for end in 1..bytes.len() {
        let prefix = bytes.get(..end).unwrap();
        // Some prefixes end on a field boundary and decode to a shorter
        // message; none may panic.
        let _ = AppendSamplesRequest::decode(prefix);
    }
}
