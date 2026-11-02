//! Parsing the filter language: `field op literal`, joined by ` AND `.

use super::{Clause, Literal, Op, names};

/// Splits on ` AND ` outside double quotes. Borrows from `text`.
pub(super) fn split_and(text: &str) -> Vec<&str> {
    let (mut parts, mut start, mut quoted) = (Vec::new(), 0, false);
    for (at, c) in text.char_indices() {
        if c == '"' {
            quoted = !quoted;
        } else if !quoted && text.get(at..).is_some_and(|t| t.starts_with(" AND ")) {
            parts.extend(text.get(start..at));
            start = at + 5;
        }
    }
    parts.extend(text.get(start..));
    parts
}

/// Finds the operator outside quotes; `<=` before `<`.
fn split_op(part: &str) -> Option<(&str, Op, &str)> {
    const OPS: [(&str, Op); 6] = [
        ("!=", Op::Ne),
        ("<=", Op::Le),
        (">=", Op::Ge),
        ("=", Op::Eq),
        ("<", Op::Lt),
        (">", Op::Gt),
    ];
    let quote = part.find('"').unwrap_or(part.len());
    let head = part.get(..quote)?;
    let (at, word, op) = OPS
        .iter()
        .filter_map(|(word, op)| head.find(word).map(|at| (at, *word, *op)))
        .min_by_key(|(at, word, _)| (*at, usize::MAX - word.len()))?;
    Some((part.get(..at)?, op, part.get(at + word.len()..)?))
}

fn literal(text: &str) -> Option<Literal> {
    if let Some(quoted) = text.strip_prefix('"') {
        let inner = quoted.strip_suffix('"').filter(|t| !t.contains('"'))?;
        return Some(Literal::Text(inner.to_owned()));
    }
    match text {
        "true" => return Some(Literal::Bool(true)),
        "false" => return Some(Literal::Bool(false)),
        _ => {}
    }
    if !text.starts_with(|c: char| c.is_ascii_digit() || matches!(c, '-' | '+' | '.')) {
        return None;
    }
    if let Ok(n) = text.parse::<i128>() {
        return Some(Literal::Int(n));
    }
    text.parse::<f64>().ok().map(Literal::Float)
}

/// Parses one clause; the error is text for the client.
pub(super) fn clause(part: &str) -> Result<Clause, String> {
    let part = part.trim();
    let shape = || format!("`{part}` is not `field op literal`");
    let (field, op, rhs) = split_op(part).ok_or_else(shape)?;
    let (field, rhs) = (field.trim(), rhs.trim());
    match field {
        "value" => {
            let value = literal(rhs).ok_or_else(|| format!("`{rhs}` is not a literal"))?;
            Ok(Clause::Value(op, value))
        }
        "quality.severity" => names::severity(rhs)
            .map(|s| Clause::Severity(op, s))
            .ok_or_else(|| format!("`{rhs}` is not a QualitySeverity value")),
        "quality.substatus" => names::substatus(rhs)
            .map(|s| Clause::Substatus(op, s))
            .ok_or_else(|| format!("`{rhs}` is not a QualitySubstatus value")),
        _ => Err(format!(
            "`{field}` cannot be filtered on: use value, quality.severity or quality.substatus"
        )),
    }
}
