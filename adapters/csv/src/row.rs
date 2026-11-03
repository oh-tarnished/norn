//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One row, from text to a sample.

use norn_adapter::{
    AdapterError, Discovery, SourceBatch, parse_severity, parse_substatus, parse_value_type, time,
};
use norn_protocol::common::{Quality, Sample, Value};

use crate::value::parse_value;
use crate::{FORMAT, record};

pub(crate) fn malformed(row: Option<u64>, detail: impl Into<String>) -> AdapterError {
    AdapterError::Malformed {
        format: FORMAT,
        row,
        detail: detail.into(),
    }
}

/// Parses `line`, row `row` of the file, appending to `out`. Copies what it keeps.
pub(crate) fn parse(
    line: &[u8],
    row: u64,
    discovery: &mut Discovery,
    out: &mut SourceBatch,
) -> Result<(), AdapterError> {
    let at_row = Some(row);
    let text = std::str::from_utf8(line).map_err(|_| malformed(at_row, "not UTF-8"))?;
    let body = text.strip_suffix('\n').unwrap_or(text);
    let body = body.strip_suffix('\r').unwrap_or(body);
    let fields = record::fields(body).map_err(|e| malformed(at_row, e))?;
    let [
        at,
        signal,
        stream,
        kind,
        value,
        severity,
        substatus,
        sequence,
    ] = fields.as_slice()
    else {
        let n = fields.len();
        return Err(malformed(at_row, format!("{n} columns, not 8")));
    };
    let bad = |what: &str, text: &str| malformed(at_row, format!("{what}: {text:?}"));
    let at = time::parse_rfc3339(at).ok_or_else(|| bad("time", at))?;
    let kind_of = parse_value_type(kind).ok_or_else(|| bad("type", kind))?;
    let value = parse_value(value, kind_of).map_err(|e| malformed(at_row, e))?;
    let quality = Quality {
        severity: parse_severity(severity).ok_or_else(|| bad("severity", severity))?,
        substatus: parse_substatus(substatus).ok_or_else(|| bad("substatus", substatus))?,
    };
    let sequence = sequence.parse().map_err(|_| bad("sequence", sequence))?;
    out.signals
        .extend(discovery.observe(signal, kind_of, stream, row)?);
    out.samples.push(Sample {
        signal: signal.to_string(),
        observe_time: Some(time::from_nanos(at)),
        value: Some(Value { kind: Some(value) }),
        quality: Some(quality),
        sequence,
    });
    Ok(())
}
