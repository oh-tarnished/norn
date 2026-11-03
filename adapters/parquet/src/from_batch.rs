//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reading rows out of a batch.

use arrow_array::cast::AsArray;
use arrow_array::types::{
    Float32Type, Float64Type, Int64Type, TimestampNanosecondType, UInt64Type,
};
use arrow_array::{Array, RecordBatch};
use norn_adapter::{
    AdapterError, Discovery, SourceBatch, parse_severity, parse_substatus, parse_value_type, time,
};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{EnumValue, Quality, Sample, Value, ValueType};

fn malformed(format: &'static str, row: u64, detail: String) -> AdapterError {
    AdapterError::Malformed {
        format,
        row: Some(row),
        detail,
    }
}

/// The value of row `i`, or why not. `Ok(None)` means its column is null.
#[allow(clippy::too_many_lines, reason = "one arm per value type")]
fn value(batch: &RecordBatch, kind: ValueType, i: usize) -> Result<Option<Kind>, String> {
    let column = |name: &str| {
        let found = batch.column_by_name(name);
        found.ok_or_else(|| format!("no column {name}"))
    };
    let bad = |name: &str| format!("column {name} has the wrong type");
    Ok(match kind {
        ValueType::Bool => {
            let c = column("bool_value")?;
            let c = c.as_boolean_opt().ok_or_else(|| bad("bool_value"))?;
            (c.is_valid(i)).then(|| Kind::BoolValue(c.value(i)))
        }
        ValueType::Int32 | ValueType::Int64 | ValueType::Uint32 | ValueType::Enum => {
            let c = column("int_value")?;
            let c = c
                .as_primitive_opt::<Int64Type>()
                .ok_or_else(|| bad("int_value"))?;
            if !c.is_valid(i) {
                return Ok(None);
            }
            let v = c.value(i);
            let narrow = |what: &str| format!("{v} does not fit {what}");
            Some(match kind {
                ValueType::Int32 => {
                    Kind::Int32Value(i32::try_from(v).map_err(|_| narrow("int32"))?)
                }
                ValueType::Uint32 => {
                    u32::try_from(v).map_err(|_| narrow("uint32"))?;
                    Kind::Uint32Value(v)
                }
                ValueType::Enum => Kind::EnumValue(EnumValue {
                    number: i32::try_from(v).map_err(|_| narrow("enum"))?,
                    symbol: String::new(),
                }),
                _ => Kind::Int64Value(v),
            })
        }
        ValueType::Uint64 => {
            let c = column("uint_value")?;
            let c = c
                .as_primitive_opt::<UInt64Type>()
                .ok_or_else(|| bad("uint_value"))?;
            (c.is_valid(i)).then(|| Kind::Uint64Value(i64::from_ne_bytes(c.value(i).to_ne_bytes())))
        }
        ValueType::Float => {
            let c = column("float_value")?;
            let c = c
                .as_primitive_opt::<Float32Type>()
                .ok_or_else(|| bad("float_value"))?;
            (c.is_valid(i)).then(|| Kind::FloatValue(c.value(i)))
        }
        ValueType::Double => {
            let c = column("double_value")?;
            let c = c
                .as_primitive_opt::<Float64Type>()
                .ok_or_else(|| bad("double_value"))?;
            (c.is_valid(i)).then(|| Kind::DoubleValue(c.value(i)))
        }
        ValueType::String => {
            let c = column("string_value")?;
            let c = c
                .as_string_opt::<i32>()
                .ok_or_else(|| bad("string_value"))?;
            (c.is_valid(i)).then(|| Kind::StringValue(c.value(i).to_owned()))
        }
        ValueType::Bytes => {
            let c = column("bytes_value")?;
            let c = c.as_binary_opt::<i32>().ok_or_else(|| bad("bytes_value"))?;
            (c.is_valid(i)).then(|| Kind::BytesValue(c.value(i).to_vec()))
        }
        ValueType::Unspecified => None,
    })
}

/// The rows of `batch`, the first being row `first` of the file. Copies them.
///
/// # Errors
/// [`AdapterError::Malformed`] naming the row, or the missing column.
pub(crate) fn from_batch(
    batch: &RecordBatch,
    first: u64,
    format: &'static str,
    discovery: &mut Discovery,
) -> Result<SourceBatch, AdapterError> {
    let columnar = |detail: String| malformed(format, first, detail);
    let text = |name: &str| {
        let found = batch
            .column_by_name(name)
            .and_then(|c| c.as_string_opt::<i32>());
        found.ok_or_else(|| columnar(format!("no text column {name}")))
    };
    let (signal, stream, kinds) = (text("signal")?, text("stream")?, text("value_type")?);
    let (severity, substatus) = (text("severity")?, text("substatus")?);
    let at = batch.column_by_name("time");
    let at = at.and_then(|c| c.as_primitive_opt::<TimestampNanosecondType>());
    let at = at.ok_or_else(|| columnar("no nanosecond column time".to_owned()))?;
    let sequence = batch.column_by_name("sequence");
    let sequence = sequence.and_then(|c| c.as_primitive_opt::<Int64Type>());
    let sequence = sequence.ok_or_else(|| columnar("no integer column sequence".to_owned()))?;
    let mut out = SourceBatch {
        signals: Vec::new(),
        samples: Vec::with_capacity(batch.num_rows()),
    };
    for i in 0..batch.num_rows() {
        let row = first + i as u64;
        let word = kinds.value(i);
        let Some(kind) = parse_value_type(word) else {
            return Err(malformed(format, row, format!("value type {word:?}")));
        };
        let found = value(batch, kind, i).map_err(|e| malformed(format, row, e))?;
        let Some(found) = found else {
            return Err(malformed(format, row, format!("no {word} value")));
        };
        let quality = parse_severity(severity.value(i)).zip(parse_substatus(substatus.value(i)));
        let Some((severity, substatus)) = quality else {
            return Err(malformed(format, row, "unknown quality".to_owned()));
        };
        let name = signal.value(i);
        out.signals
            .extend(discovery.observe(name, kind, stream.value(i), row)?);
        out.samples.push(Sample {
            signal: name.to_owned(),
            observe_time: Some(time::from_nanos(at.value(i))),
            value: Some(Value { kind: Some(found) }),
            quality: Some(Quality {
                severity,
                substatus,
            }),
            sequence: sequence.value(i),
        });
    }
    Ok(out)
}
