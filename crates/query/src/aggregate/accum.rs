//! One window's running reduction. docs/reference/query.md defines each exactly.

use std::cmp::Ordering;

use norn_core::{Timestamp, Value};

use crate::spec::Function;

/// A number, exact while it can be: integers sum in `i128`, which cannot overflow
/// on any count of 64-bit values a window can hold.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Num {
    Int(i128),
    Float(f64),
}

impl Num {
    fn of(value: &Value) -> Option<Self> {
        Some(match value {
            Value::I32(v) => Self::Int(i128::from(*v)),
            Value::I64(v) => Self::Int(i128::from(*v)),
            Value::U32(v) => Self::Int(i128::from(*v)),
            Value::U64(v) => Self::Int(i128::from(*v)),
            Value::F32(v) => Self::Float(f64::from(*v)),
            Value::F64(v) => Self::Float(*v),
            _ => return None,
        })
    }

    fn float(self) -> f64 {
        match self {
            Self::Int(v) => v as f64,
            Self::Float(v) => v,
        }
    }

    /// IEEE 754 for floats. The first sample starts the sum, so a sum of `-0.0`
    /// stays `-0.0` where starting from zero would make it `+0.0`.
    fn add(self, other: Self) -> Self {
        match (self, other) {
            (Self::Int(a), Self::Int(b)) => Self::Int(a.saturating_add(b)),
            (a, b) => Self::Float(a.float() + b.float()),
        }
    }

    /// What this sample adds to a counter after `previous`: the difference, or,
    /// after a decrease, all of it, since a reset restarts from zero.
    fn increase(self, previous: Self) -> Self {
        match (self, previous) {
            (Self::Int(c), Self::Int(p)) => Self::Int(if c < p { c } else { c - p }),
            (c, p) => {
                let (c, p) = (c.float(), p.float());
                Self::Float(if c < p { c } else { c - p })
            }
        }
    }
}

/// Orders two values of one kind. Floats use IEEE 754 `totalOrder`, so `-NaN <
/// -inf < ... < -0.0 < +0.0 < ... < +inf < +NaN`, and the answer depends on no
/// sample's position. Strings and bytes order by bytes, booleans `false` first.
fn order(a: &Value, b: &Value) -> Ordering {
    match (a, b) {
        (Value::Bool(a), Value::Bool(b)) => a.cmp(b),
        (Value::I32(a), Value::I32(b)) => a.cmp(b),
        (Value::I64(a), Value::I64(b)) => a.cmp(b),
        (Value::U32(a), Value::U32(b)) | (Value::Enum(a), Value::Enum(b)) => a.cmp(b),
        (Value::U64(a), Value::U64(b)) => a.cmp(b),
        (Value::F32(a), Value::F32(b)) => a.total_cmp(b),
        (Value::F64(a), Value::F64(b)) => a.total_cmp(b),
        (Value::String(a), Value::String(b)) => a.cmp(b),
        (Value::Bytes(a), Value::Bytes(b)) => a.cmp(b),
        _ => Ordering::Equal,
    }
}

/// The state of one window. Fed in (time, sequence) order.
///
/// Memory: at most two values. Concurrency: owned by one query.
#[derive(Debug, Clone)]
pub(crate) struct Accum {
    function: Function,
    count: u64,
    kept: Option<Value>,
    sum: Option<Num>,
    previous: Option<Num>,
    first_time: Option<Timestamp>,
    last_time: Option<Timestamp>,
}

impl Accum {
    pub(crate) fn new(function: Function) -> Self {
        Self {
            function,
            count: 0,
            kept: None,
            sum: None,
            previous: None,
            first_time: None,
            last_time: None,
        }
    }

    /// Feeds one sample.
    pub(crate) fn add(&mut self, time: Timestamp, value: &Value) {
        self.count += 1;
        self.first_time.get_or_insert(time);
        self.last_time = Some(time);
        match self.function {
            Function::First => {
                self.kept.get_or_insert_with(|| value.clone());
            }
            Function::Last | Function::Latest => self.kept = Some(value.clone()),
            Function::Min | Function::Max => self.keep_extreme(value),
            Function::Average | Function::Sum => {
                let n = Num::of(value);
                self.sum = match (self.sum, n) {
                    (Some(s), Some(n)) => Some(s.add(n)),
                    (s, n) => s.or(n),
                };
            }
            Function::Rate => self.add_rate(value),
            Function::Count => {}
        }
    }

    fn keep_extreme(&mut self, value: &Value) {
        let wanted = if self.function == Function::Min {
            Ordering::Less
        } else {
            Ordering::Greater
        };
        if self.kept.as_ref().is_none_or(|k| order(value, k) == wanted) {
            self.kept = Some(value.clone());
        }
    }

    fn add_rate(&mut self, value: &Value) {
        let Some(n) = Num::of(value) else { return };
        if let Some(previous) = self.previous {
            let gained = n.increase(previous);
            self.sum = Some(self.sum.map_or(gained, |s| s.add(gained)));
        }
        self.previous = Some(n);
    }

    /// Samples reduced.
    pub(crate) fn count(&self) -> u64 {
        self.count
    }

    /// The result; `None` when the window has none. RATE needs two samples at
    /// different times. A NaN result is a NaN, its payload unspecified.
    pub(crate) fn finish(self) -> Option<Value> {
        match self.function {
            Function::Count => i64::try_from(self.count).ok().map(Value::I64),
            Function::Sum => self.sum.map(|s| Value::F64(s.float())),
            Function::Average => {
                let sum = self.sum?.float();
                Some(Value::F64(sum / self.count as f64))
            }
            Function::Rate => {
                let nanos = self.last_time?.nanos_since(self.first_time?).ok()?;
                let seconds = nanos as f64 / 1e9;
                (nanos > 0).then(|| Value::F64(self.sum.map_or(0.0, Num::float) / seconds))
            }
            _ => self.kept,
        }
    }
}
