//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Samples as columns, and back, exactly.

use std::collections::HashMap;
use std::sync::Arc;

use arrow_array::builder::{
    BinaryBuilder, BooleanBuilder, Float32Builder, Float64Builder, Int64Builder, StringBuilder,
    TimestampNanosecondBuilder, UInt64Builder,
};
use arrow_array::{ArrayRef, RecordBatch};
use arrow_schema::{ArrowError, DataType, Field, Schema, TimeUnit};
use norn_adapter::{
    AdapterError, severity_name, substatus_name, time, value_type_name, value_type_of,
};
use norn_protocol::common::Sample;
use norn_protocol::common::value::Kind;

/// The time zone of the `time` column: Norn's instants are UTC.
const UTC: &str = "UTC";

/// The columns. `int_value` holds `int32`, `int64`, `uint32` and `enum`, which it
/// covers exactly; `uint_value` holds `uint64`, whose top half it alone covers.
#[must_use]
pub fn schema() -> Schema {
    let text = |name| Field::new(name, DataType::Utf8, false);
    Schema::new(vec![
        text("signal"),
        text("stream"),
        text("value_type"),
        Field::new(
            "time",
            DataType::Timestamp(TimeUnit::Nanosecond, Some(UTC.into())),
            false,
        ),
        Field::new("sequence", DataType::Int64, false),
        text("severity"),
        text("substatus"),
        Field::new("bool_value", DataType::Boolean, true),
        Field::new("int_value", DataType::Int64, true),
        Field::new("uint_value", DataType::UInt64, true),
        Field::new("float_value", DataType::Float32, true),
        Field::new("double_value", DataType::Float64, true),
        Field::new("string_value", DataType::Utf8, true),
        Field::new("bytes_value", DataType::Binary, true),
    ])
}

/// `samples` as a batch of [`schema`], copying them. `streams` maps each signal to
/// its stream, from the catalog.
///
/// # Errors
/// [`AdapterError::Encode`] for a signal not in `streams`, or a sample missing a
/// required field.
pub fn to_batch(
    samples: &[Sample],
    streams: &HashMap<String, String>,
    format: &'static str,
) -> Result<RecordBatch, AdapterError> {
    let encode = |detail: String| AdapterError::Encode { format, detail };
    let n = samples.len();
    let (mut signal, mut stream) = (StringBuilder::new(), StringBuilder::new());
    let (mut value_type, mut severity) = (StringBuilder::new(), StringBuilder::new());
    let mut substatus = StringBuilder::new();
    let mut at = TimestampNanosecondBuilder::with_capacity(n).with_timezone(UTC);
    let mut sequence = Int64Builder::with_capacity(n);
    let mut bools = BooleanBuilder::with_capacity(n);
    let mut ints = Int64Builder::with_capacity(n);
    let mut uints = UInt64Builder::with_capacity(n);
    let mut floats = Float32Builder::with_capacity(n);
    let mut doubles = Float64Builder::with_capacity(n);
    let mut strings = StringBuilder::new();
    let mut bytes = BinaryBuilder::new();
    for s in samples {
        let Some(owner) = streams.get(&s.signal) else {
            return Err(encode(format!("{} is not in the catalog", s.signal)));
        };
        let instant = s.observe_time.as_ref().and_then(time::to_nanos);
        let instant = instant.ok_or_else(|| encode("a sample has no time".to_owned()))?;
        let kind = s.value.as_ref().and_then(|v| v.kind.as_ref());
        let kind = kind.ok_or_else(|| encode("a sample has no value".to_owned()))?;
        let quality = s.quality.unwrap_or_default();
        signal.append_value(&s.signal);
        stream.append_value(owner);
        value_type.append_value(value_type_name(value_type_of(kind)));
        at.append_value(instant);
        sequence.append_value(s.sequence);
        severity.append_value(severity_name(quality.severity));
        substatus.append_value(substatus_name(quality.substatus));
        let int = match kind {
            Kind::Int32Value(v) => Some(i64::from(*v)),
            Kind::Int64Value(v) | Kind::Uint32Value(v) => Some(*v),
            Kind::EnumValue(v) => Some(i64::from(v.number)),
            _ => None,
        };
        ints.append_option(int);
        bools.append_option(if let Kind::BoolValue(v) = kind {
            Some(*v)
        } else {
            None
        });
        uints.append_option(if let Kind::Uint64Value(v) = kind {
            Some(u64::from_ne_bytes(v.to_ne_bytes()))
        } else {
            None
        });
        floats.append_option(if let Kind::FloatValue(v) = kind {
            Some(*v)
        } else {
            None
        });
        doubles.append_option(if let Kind::DoubleValue(v) = kind {
            Some(*v)
        } else {
            None
        });
        strings.append_option(if let Kind::StringValue(v) = kind {
            Some(v.as_str())
        } else {
            None
        });
        bytes.append_option(if let Kind::BytesValue(v) = kind {
            Some(v.as_slice())
        } else {
            None
        });
    }
    let columns: Vec<ArrayRef> = vec![
        Arc::new(signal.finish()),
        Arc::new(stream.finish()),
        Arc::new(value_type.finish()),
        Arc::new(at.finish()),
        Arc::new(sequence.finish()),
        Arc::new(severity.finish()),
        Arc::new(substatus.finish()),
        Arc::new(bools.finish()),
        Arc::new(ints.finish()),
        Arc::new(uints.finish()),
        Arc::new(floats.finish()),
        Arc::new(doubles.finish()),
        Arc::new(strings.finish()),
        Arc::new(bytes.finish()),
    ];
    RecordBatch::try_new(Arc::new(schema()), columns).map_err(|e: ArrowError| encode(e.to_string()))
}
