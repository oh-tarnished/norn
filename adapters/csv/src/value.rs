//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Values as text, exact in both directions.

use std::fmt::Write;

use norn_protocol::common::value::Kind;
use norn_protocol::common::{EnumValue, ValueType};

fn hex(bytes: &[u8], out: &mut String) {
    for b in bytes {
        let _ = write!(out, "{b:02x}");
    }
}

fn unhex(text: &str) -> Result<Vec<u8>, String> {
    let (pairs, odd) = text.as_bytes().as_chunks::<2>();
    if !odd.is_empty() {
        return Err("hex of odd length".to_owned());
    }
    let digit = |c: u8| {
        char::from(c)
            .to_digit(16)
            .and_then(|d| u8::try_from(d).ok())
            .ok_or_else(|| format!("{:?} is not a hex digit", char::from(c)))
    };
    let bytes = pairs
        .iter()
        .map(|[hi, lo]| Ok(digit(*hi)? << 4 | digit(*lo)?));
    bytes.collect()
}

/// Appends `kind` as text, unquoted; the caller quotes it.
pub(crate) fn write_value(kind: &Kind, out: &mut String) {
    let _ = match kind {
        Kind::BoolValue(v) => write!(out, "{v}"),
        Kind::Int32Value(v) => write!(out, "{v}"),
        Kind::Int64Value(v) | Kind::Uint32Value(v) => write!(out, "{v}"),
        Kind::Uint64Value(v) => write!(out, "{}", u64::from_ne_bytes(v.to_ne_bytes())),
        Kind::FloatValue(v) if v.to_bits() == f32::NAN.to_bits() => out.write_str("NaN"),
        Kind::FloatValue(v) if v.is_nan() => write!(out, "nan:{:08x}", v.to_bits()),
        Kind::FloatValue(v) => write!(out, "{v}"),
        Kind::DoubleValue(v) if v.to_bits() == f64::NAN.to_bits() => out.write_str("NaN"),
        Kind::DoubleValue(v) if v.is_nan() => write!(out, "nan:{:016x}", v.to_bits()),
        Kind::DoubleValue(v) => write!(out, "{v}"),
        Kind::StringValue(v) => out.write_str(v),
        Kind::BytesValue(v) => {
            hex(v, out);
            Ok(())
        }
        Kind::EnumValue(v) => write!(out, "{}", v.number),
    };
}

fn number<T: std::str::FromStr>(text: &str, what: &str) -> Result<T, String> {
    text.parse()
        .map_err(|_| format!("{text:?} is not a {what}"))
}

fn exact(text: &str) -> bool {
    !text.is_empty() && !text.starts_with('+') && !text.contains(char::is_whitespace)
}

fn parse_f64(text: &str) -> Result<f64, String> {
    if text == "NaN" {
        return Ok(f64::NAN);
    }
    if let Some(bits) = text.strip_prefix("nan:") {
        let bits = u64::from_str_radix(bits, 16).map_err(|e| e.to_string())?;
        return Ok(f64::from_bits(bits));
    }
    let parsed: f64 = number(text, "double")?;
    if parsed.is_nan() || !exact(text) {
        return Err(format!("{text:?} is not a double"));
    }
    Ok(parsed)
}

fn parse_f32(text: &str) -> Result<f32, String> {
    if text == "NaN" {
        return Ok(f32::NAN);
    }
    if let Some(bits) = text.strip_prefix("nan:") {
        let bits = u32::from_str_radix(bits, 16).map_err(|e| e.to_string())?;
        return Ok(f32::from_bits(bits));
    }
    let parsed: f32 = number(text, "float")?;
    if parsed.is_nan() || !exact(text) {
        return Err(format!("{text:?} is not a float"));
    }
    Ok(parsed)
}

/// The value `text` holds for a column of type `kind`. Copies; `Err` says why.
pub(crate) fn parse_value(text: &str, kind: ValueType) -> Result<Kind, String> {
    Ok(match kind {
        ValueType::Bool => match text {
            "true" => Kind::BoolValue(true),
            "false" => Kind::BoolValue(false),
            _ => return Err(format!("{text:?} is not true or false")),
        },
        ValueType::Int32 => Kind::Int32Value(number(text, "int32")?),
        ValueType::Int64 => Kind::Int64Value(number(text, "int64")?),
        ValueType::Uint32 => Kind::Uint32Value(i64::from(number::<u32>(text, "uint32")?)),
        ValueType::Uint64 => {
            let v: u64 = number(text, "uint64")?;
            Kind::Uint64Value(i64::from_ne_bytes(v.to_ne_bytes()))
        }
        ValueType::Float => Kind::FloatValue(parse_f32(text)?),
        ValueType::Double => Kind::DoubleValue(parse_f64(text)?),
        ValueType::String => Kind::StringValue(text.to_owned()),
        ValueType::Bytes => Kind::BytesValue(unhex(text)?),
        ValueType::Enum => Kind::EnumValue(EnumValue {
            number: number::<i32>(text, "enum number")?,
            symbol: String::new(),
        }),
        ValueType::Unspecified => return Err("no value type".to_owned()),
    })
}
