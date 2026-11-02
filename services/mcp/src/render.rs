//! Protocol messages as the JSON a model reads, each row bounded.
//!
//! Numbers stay JSON numbers, exact where JSON's are. What JSON cannot say is
//! spelled: `"NaN"`, `"Infinity"` and `"-Infinity"`. A string or bytes value past
//! [`MAX_VALUE_CHARS`] is cut, and its row says `truncated`: never a silent loss, and
//! no sample can make a result unbounded.

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, QualitySeverity, QualitySubstatus, Sample, Value};
use norn_protocol::query::Aggregate;
use prost::Message;
use prost_types::Timestamp;
use serde_json::{Map, Value as Json, json};

use crate::cursor::Key;
use crate::time;

/// The most JSON-escaped characters of one string or bytes value in a result.
pub(crate) const MAX_VALUE_CHARS: usize = 1_024;

fn float(v: f64) -> Json {
    if v.is_nan() {
        json!("NaN")
    } else if v.is_infinite() {
        json!(if v > 0.0 { "Infinity" } else { "-Infinity" })
    } else {
        json!(v)
    }
}

/// `v` cut to [`MAX_VALUE_CHARS`] as JSON will escape it, on a character boundary.
fn cut(text: &str) -> (String, bool) {
    let mut used = 0;
    for (at, c) in text.char_indices() {
        used += match c {
            '"' | '\\' => 2,
            c if (c as u32) < 0x20 => 6,
            c => c.len_utf8(),
        };
        if used > MAX_VALUE_CHARS {
            return (text.get(..at).unwrap_or_default().to_owned(), true);
        }
    }
    (text.to_owned(), false)
}

/// A value, and whether it was cut.
pub(crate) fn value(v: Option<&Value>) -> (Json, bool) {
    let Some(kind) = v.and_then(|v| v.kind.as_ref()) else {
        return (Json::Null, false);
    };
    let plain = match kind {
        Kind::BoolValue(v) => json!(v),
        Kind::Int32Value(v) => json!(v),
        Kind::Int64Value(v) | Kind::Uint32Value(v) => json!(v),
        Kind::Uint64Value(v) => json!(*v as u64),
        // Through its shortest text: widening would show 0.1 as 0.10000000149.
        Kind::FloatValue(v) if v.is_finite() => float(v.to_string().parse().unwrap_or(f64::NAN)),
        Kind::FloatValue(v) => float(f64::from(*v)),
        Kind::DoubleValue(v) => float(*v),
        Kind::StringValue(v) => {
            let (text, cut) = cut(v);
            return (json!(text), cut);
        }
        Kind::BytesValue(v) => {
            let shown = v.iter().take(MAX_VALUE_CHARS / 2);
            let hex: String = shown.map(|b| format!("{b:02x}")).collect();
            return (json!(hex), v.len() > MAX_VALUE_CHARS / 2);
        }
        Kind::EnumValue(v) if v.symbol.is_empty() => json!(v.number),
        Kind::EnumValue(v) => json!({"number": v.number, "symbol": v.symbol}),
    };
    (plain, false)
}

/// A number from a value, for arithmetic between results.
pub(crate) fn number(v: Option<&Value>) -> Option<f64> {
    Some(match v?.kind.as_ref()? {
        Kind::Int32Value(v) => f64::from(*v),
        Kind::Int64Value(v) | Kind::Uint32Value(v) => *v as f64,
        Kind::Uint64Value(v) => (*v as u64) as f64,
        Kind::FloatValue(v) => f64::from(*v),
        Kind::DoubleValue(v) => *v,
        _ => return None,
    })
}

/// A number as a result shows it.
pub(crate) fn number_json(v: f64) -> Json {
    float(v)
}

/// A hash of a value's exact bits: -0.0 differs from 0.0, and a NaN equals itself. An
/// enum is its number, as Norn compares it; its symbol is for display.
pub(crate) fn bits(v: Option<&Value>) -> u64 {
    let bytes = match v.and_then(|v| v.kind.as_ref()) {
        Some(Kind::EnumValue(e)) => e.number.to_le_bytes().to_vec(),
        _ => v.map(Message::encode_to_vec).unwrap_or_default(),
    };
    fnv(&bytes)
}

/// FNV-1a.
pub(crate) fn fnv(bytes: &[u8]) -> u64 {
    let start = 0xcbf2_9ce4_8422_2325u64;
    bytes.iter().fold(start, |h, b| {
        (h ^ u64::from(*b)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

/// A quality as `GOOD` or `BAD/SENSOR_FAILURE`.
pub(crate) fn quality(q: Option<&Quality>) -> String {
    let q = q.copied().unwrap_or_default();
    let severity = QualitySeverity::try_from(q.severity).unwrap_or(QualitySeverity::Unspecified);
    let severity = severity
        .as_str_name()
        .trim_start_matches("QUALITY_SEVERITY_");
    match QualitySubstatus::try_from(q.substatus) {
        Ok(QualitySubstatus::Unspecified) | Err(_) => severity.to_owned(),
        Ok(s) => {
            let detail = s.as_str_name().trim_start_matches("QUALITY_SUBSTATUS_");
            format!("{severity}/{detail}")
        }
    }
}

/// A time as a result shows it; `null` when unset.
pub(crate) fn when(t: Option<Timestamp>) -> Json {
    t.map_or(Json::Null, |t| json!(time::format(t)))
}

/// Sets `key` on the object `row`.
pub(crate) fn set(row: &mut Json, key: &str, v: Json) {
    if let Some(map) = row.as_object_mut() {
        map.insert(key.to_owned(), v);
    }
}

/// One sample.
pub(crate) fn sample(s: &Sample) -> Json {
    let (shown, cut) = value(s.value.as_ref());
    let mut row = Map::new();
    row.insert("signal".to_owned(), json!(s.signal));
    row.insert("time".to_owned(), when(s.observe_time));
    row.insert("value".to_owned(), shown);
    row.insert("quality".to_owned(), json!(quality(s.quality.as_ref())));
    row.insert("sequence".to_owned(), json!(s.sequence));
    if cut {
        row.insert("truncated".to_owned(), json!(true));
    }
    Json::Object(row)
}

/// One aggregate: `start` and `end` are the window, `null` where it is open.
pub(crate) fn aggregate(a: &Aggregate) -> Json {
    let (shown, cut) = value(a.value.as_ref());
    let window = a.window.unwrap_or_default();
    let mut row = json!({
        "signal": a.signal,
        "start": when(window.start_time),
        "end": when(window.end_time),
        "value": shown,
        "sample_count": a.sample_count,
    });
    if cut {
        set(&mut row, "truncated", json!(true));
    }
    row
}

fn nanos(t: Option<Timestamp>) -> i64 {
    t.map_or(0, |t| i64::try_from(time::nanos(t)).unwrap_or(i64::MAX))
}

/// A sample's position.
pub(crate) fn sample_key(s: &Sample) -> Key {
    Key {
        n: s.signal.clone(),
        t: nanos(s.observe_time),
        s: s.sequence,
    }
}

/// An aggregate's position: its signal and window.
pub(crate) fn aggregate_key(a: &Aggregate) -> Key {
    Key {
        n: a.signal.clone(),
        t: nanos(a.window.unwrap_or_default().start_time),
        s: 0,
    }
}
