//! Comparing a stored value with a filter's literal.

use std::cmp::Ordering;

use norn_core::{Value, ValueKind};

use super::{Literal, Op};

/// An integer as `i128`, exact for every kind.
fn integer(value: &Value) -> Option<i128> {
    Some(match value {
        Value::I32(v) => i128::from(*v),
        Value::I64(v) => i128::from(*v),
        Value::U32(v) | Value::Enum(v) => i128::from(*v),
        Value::U64(v) => i128::from(*v),
        _ => return None,
    })
}

/// How `value` orders against `literal`. `None` when unordered (a NaN) or when
/// they cannot compare, which [`applies`] has ruled out for a running query.
pub(super) fn compare(value: &Value, literal: &Literal) -> Option<Ordering> {
    let float = |v: f64| match literal {
        Literal::Int(n) => v.partial_cmp(&(*n as f64)),
        Literal::Float(l) => v.partial_cmp(l),
        _ => None,
    };
    match value {
        Value::F32(v) => float(f64::from(*v)),
        Value::F64(v) => float(*v),
        Value::Bool(v) => match literal {
            Literal::Bool(l) => Some(v.cmp(l)),
            _ => None,
        },
        Value::String(v) => match literal {
            Literal::Text(l) => Some(v.as_str().cmp(l.as_str())),
            _ => None,
        },
        Value::Bytes(v) => match literal {
            Literal::Text(l) => Some(v.as_slice().cmp(l.as_bytes())),
            _ => None,
        },
        other => match (integer(other)?, literal) {
            (v, Literal::Int(l)) => Some(v.cmp(l)),
            (v, Literal::Float(l)) => (v as f64).partial_cmp(l),
            _ => None,
        },
    }
}

/// Whether `op literal` means anything on values of `kind`.
pub(super) fn applies(op: Op, literal: &Literal, kind: ValueKind) -> bool {
    match (kind, literal) {
        (ValueKind::Bool, Literal::Bool(_)) => matches!(op, Op::Eq | Op::Ne),
        (ValueKind::String | ValueKind::Bytes, Literal::Text(_)) => true,
        (
            ValueKind::I32
            | ValueKind::I64
            | ValueKind::U32
            | ValueKind::U64
            | ValueKind::F32
            | ValueKind::F64
            | ValueKind::Enum,
            Literal::Int(_) | Literal::Float(_),
        ) => true,
        _ => false,
    }
}
