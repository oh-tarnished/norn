//! Generators of awkward samples.

use norn_adapter::time;
use norn_protocol::common::value::Kind;
use norn_protocol::common::{EnumValue, Quality, Sample, Value, ValueType};
use proptest::prelude::*;

use crate::signal_name;

/// The doubles that a text or columnar format most easily changes.
pub const AWKWARD_DOUBLES: [u64; 8] = [
    0x0000_0000_0000_0000, // +0.0
    0x8000_0000_0000_0000, // -0.0
    0x7ff0_0000_0000_0000, // +inf
    0xfff0_0000_0000_0000, // -inf
    0x7ff8_0000_0000_0000, // the canonical quiet NaN
    0xfff8_0000_0000_0000, // a negative NaN
    0x7ff0_0000_0000_0001, // a signaling NaN, payload 1
    0x7ff8_dead_beef_cafe, // a quiet NaN with a payload
];

/// The floats that a text or columnar format most easily changes.
pub const AWKWARD_FLOATS: [u32; 8] = [
    0x0000_0000,
    0x8000_0000,
    0x7f80_0000,
    0xff80_0000,
    0x7fc0_0000,
    0xffc0_0000,
    0x7f80_0001,
    0x7fc0_beef,
];

/// Any value, weighted towards extremes, with the [`ValueType`] it belongs to.
pub fn value() -> impl Strategy<Value = (ValueType, Kind)> {
    let awkward_double =
        (0..AWKWARD_DOUBLES.len()).prop_map(|i| AWKWARD_DOUBLES.get(i).copied().unwrap_or(0));
    let awkward_float =
        (0..AWKWARD_FLOATS.len()).prop_map(|i| AWKWARD_FLOATS.get(i).copied().unwrap_or(0));
    prop_oneof![
        any::<bool>().prop_map(|v| (ValueType::Bool, Kind::BoolValue(v))),
        prop_oneof![Just(i32::MIN), Just(i32::MAX), any::<i32>()]
            .prop_map(|v| (ValueType::Int32, Kind::Int32Value(v))),
        prop_oneof![Just(i64::MIN), Just(i64::MAX), any::<i64>()]
            .prop_map(|v| (ValueType::Int64, Kind::Int64Value(v))),
        prop_oneof![Just(0u32), Just(u32::MAX), any::<u32>()]
            .prop_map(|v| (ValueType::Uint32, Kind::Uint32Value(i64::from(v)))),
        prop_oneof![Just(0u64), Just(u64::MAX), Just(1u64 << 63), any::<u64>()].prop_map(|v| (
            ValueType::Uint64,
            Kind::Uint64Value(i64::from_ne_bytes(v.to_ne_bytes()))
        )),
        prop_oneof![awkward_float, any::<u32>()]
            .prop_map(|b| (ValueType::Float, Kind::FloatValue(f32::from_bits(b)))),
        prop_oneof![awkward_double, any::<u64>()]
            .prop_map(|b| (ValueType::Double, Kind::DoubleValue(f64::from_bits(b)))),
        prop_oneof![
            Just(String::new()),
            Just("a,\"b\"\r\nc ".to_owned()),
            any::<String>()
        ]
        .prop_map(|v| (ValueType::String, Kind::StringValue(v))),
        proptest::collection::vec(any::<u8>(), 0..32)
            .prop_map(|v| (ValueType::Bytes, Kind::BytesValue(v))),
        (0..=i32::MAX).prop_map(|n| {
            (
                ValueType::Enum,
                Kind::EnumValue(EnumValue {
                    number: n,
                    symbol: String::new(),
                }),
            )
        }),
    ]
}

/// Any quality Norn accepts: all severities, all substatuses.
pub fn quality() -> impl Strategy<Value = Quality> {
    (1..=3i32, 0..=11i32).prop_map(|(severity, substatus)| Quality {
        severity,
        substatus,
    })
}

/// A sample of the [`crate::catalog`] signal of its value's type. `time` is any instant
/// within `times`, so the caller picks distinct ones.
pub fn sample(times: std::ops::Range<i64>) -> impl Strategy<Value = Sample> {
    (value(), quality(), times, 0..i64::MAX).prop_map(|((kind, value), quality, at, sequence)| {
        Sample {
            signal: signal_name(kind),
            observe_time: Some(time::from_nanos(at)),
            value: Some(Value { kind: Some(value) }),
            quality: Some(quality),
            sequence,
        }
    })
}
