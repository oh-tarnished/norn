//! Sample predicates: the `sample_filter` of a query.

mod names;
mod parse;
mod test_value;

use std::cmp::Ordering;
use std::fmt;

use norn_core::{Quality, Severity, Substatus, Value, ValueKind};

/// The longest filter parsed; a longer one is refused before any work.
const MAX_FILTER_BYTES: usize = 4096;

/// Why a filter does not parse. `Display` is for the client.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct FilterError {
    /// What is wrong, naming the clause.
    pub reason: String,
}

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason)
    }
}

impl std::error::Error for FilterError {}

/// A comparison operator.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Eq,
    Ne,
    Lt,
    Le,
    Gt,
    Ge,
}

impl Op {
    /// Whether `ordering` satisfies this operator. `None` is IEEE 754's unordered
    /// result, a NaN: only `!=` holds.
    fn holds(self, ordering: Option<Ordering>) -> bool {
        let Some(o) = ordering else {
            return self == Self::Ne;
        };
        match self {
            Self::Eq => o.is_eq(),
            Self::Ne => o.is_ne(),
            Self::Lt => o.is_lt(),
            Self::Le => o.is_le(),
            Self::Gt => o.is_gt(),
            Self::Ge => o.is_ge(),
        }
    }
}

/// The right-hand side of a value comparison.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Literal {
    Int(i128),
    Float(f64),
    Bool(bool),
    Text(String),
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Clause {
    Value(Op, Literal),
    Severity(Op, Severity),
    Substatus(Op, Substatus),
}

/// Which samples count: clauses that must all hold.
///
/// `Default` matches every sample. Concurrency: immutable once parsed, `Send +
/// Sync`. Performance: evaluated per sample, quality first, because the quality
/// column is already in memory and most filters are about it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Filter {
    quality: Vec<Clause>,
    value: Vec<Clause>,
}

impl Filter {
    /// Parses AIP-160 text: clauses over `value`, `quality.severity` and
    /// `quality.substatus` joined by `AND`, such as
    /// `quality.severity = QUALITY_SEVERITY_GOOD AND value > 20.5`. Empty matches
    /// everything. Anything else is refused, never ignored. Copies what it keeps.
    ///
    /// # Errors
    /// [`FilterError`], naming the clause.
    pub fn parse(text: &str) -> Result<Self, FilterError> {
        let refuse = |reason: String| FilterError { reason };
        if text.len() > MAX_FILTER_BYTES {
            return Err(refuse("is too long".to_owned()));
        }
        let mut filter = Self::default();
        if text.trim().is_empty() {
            return Ok(filter);
        }
        for part in parse::split_and(text) {
            match parse::clause(part).map_err(refuse)? {
                c @ Clause::Value(..) => filter.value.push(c),
                c => filter.quality.push(c),
            }
        }
        Ok(filter)
    }

    /// Whether every `value` clause can apply to `kind`. A comparison that cannot
    /// mean anything is an error for the whole query, not a silent non-match.
    pub(crate) fn applies_to(&self, kind: ValueKind) -> bool {
        self.value.iter().all(|c| match c {
            Clause::Value(op, literal) => test_value::applies(*op, literal, kind),
            _ => true,
        })
    }

    /// Whether `quality` passes.
    #[must_use]
    pub fn passes_quality(&self, quality: Quality) -> bool {
        self.quality.iter().all(|c| match c {
            Clause::Severity(op, s) => op.holds(Some((quality.severity() as u8).cmp(&(*s as u8)))),
            Clause::Substatus(op, s) => {
                op.holds(Some((quality.substatus() as u8).cmp(&(*s as u8))))
            }
            Clause::Value(..) => true,
        })
    }

    /// Whether `value` passes. Comparisons are IEEE 754 for floats: a NaN passes
    /// only `!=`, and `-0.0 = 0.0`.
    #[must_use]
    pub fn passes_value(&self, value: &Value) -> bool {
        self.value.iter().all(|c| match c {
            Clause::Value(op, literal) => op.holds(test_value::compare(value, literal)),
            _ => true,
        })
    }
}
