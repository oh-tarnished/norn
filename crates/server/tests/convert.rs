//! Wire ↔ core conversions at their edges.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_core::{Quality, Sample, Sequence, Severity, SignalId, Substatus, Timestamp, Value};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{self, EnumValue};
use norn_server::convert::*;

fn wire(kind: Kind) -> common::Value {
    common::Value { kind: Some(kind) }
}

#[test]
fn uint32_is_range_checked_at_both_ends() {
    assert_eq!(
        value_from_wire(wire(Kind::Uint32Value(4_294_967_295))),
        Ok(Value::U32(u32::MAX))
    );
    let over = value_from_wire(wire(Kind::Uint32Value(1 << 32)));
    assert_eq!(over, Err(ConvertError::Uint32OutOfRange(1 << 32)));
    assert!(value_from_wire(wire(Kind::Uint32Value(-1))).is_err());
}

#[test]
fn uint64_reinterprets_the_same_bits() {
    assert_eq!(
        value_from_wire(wire(Kind::Uint64Value(-1))),
        Ok(Value::U64(u64::MAX))
    );
    let back = value_to_wire(&Value::U64(u64::MAX)).unwrap();
    assert_eq!(back, wire(Kind::Uint64Value(-1)));
}

#[test]
fn a_negative_enum_is_rejected_and_a_huge_one_is_refused_toward_the_wire() {
    let negative = wire(Kind::EnumValue(EnumValue {
        number: -1,
        symbol: String::new(),
    }));
    assert_eq!(
        value_from_wire(negative),
        Err(ConvertError::NegativeEnum(-1))
    );
    let huge = value_to_wire(&Value::Enum(1 << 31));
    assert_eq!(huge, Err(ConvertError::EnumTooLarge(1 << 31)));
}

#[test]
fn a_sequence_above_i64_max_is_refused_toward_the_wire() {
    let s = Sample::new(
        Timestamp::UNIX_EPOCH,
        SignalId::new(1),
        Value::Bool(true),
        Quality::GOOD,
        Sequence::new(u64::MAX),
    );
    let refused = sample_to_wire(&s, "x".into());
    assert_eq!(
        refused.err(),
        Some(ConvertError::SequenceTooLarge(u64::MAX))
    );
}

#[test]
fn quality_severity_sits_one_above_core_and_unspecified_is_rejected() {
    let q = Quality::new(Severity::Bad, Substatus::SensorFailure);
    let w = quality_to_wire(q);
    assert_eq!((w.severity, w.substatus), (3, 9));
    assert_eq!(quality_from_wire(w), Ok(q));
    let unset = common::Quality {
        severity: 0,
        substatus: 0,
    };
    assert_eq!(quality_from_wire(unset), Err(ConvertError::BadSeverity(0)));
    let unknown = common::Quality {
        severity: 1,
        substatus: 200,
    };
    assert!(quality_from_wire(unknown).is_err());
}

#[test]
fn value_types_match_kind_tags_and_unspecified_is_refused() {
    for kind in norn_core::ValueKind::ALL {
        assert_eq!(kind_from_wire(kind_to_wire(kind)), Ok(kind));
    }
    assert_eq!(kind_from_wire(0), Err(ConvertError::BadValueType(0)));
}

#[test]
fn times_round_trip_before_the_epoch_and_refuse_overflow() {
    let t = Timestamp::from_nanos(-1);
    assert_eq!(time_from_wire(time_to_wire(t)), Ok(t));
    let far = prost_types::Timestamp {
        seconds: i64::MAX,
        nanos: 0,
    };
    assert_eq!(time_from_wire(far), Err(ConvertError::TimeOutOfRange));
}
