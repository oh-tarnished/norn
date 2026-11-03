//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Signals found row by row, for a format with no declaration up front.

use std::collections::HashMap;

use norn_protocol::common::ValueType;
use norn_protocol::signal::Signal;

use crate::{AdapterError, entity_of};

/// What a source has seen: each signal's type and stream, so a row that
/// contradicts an earlier one is an error rather than a misread.
///
/// Holds one entry per signal, at most `max_signals`. Lookups allocate nothing,
/// so calling it per row is cheap. Used by one task at a time.
#[derive(Debug)]
pub struct Discovery {
    format: &'static str,
    known: HashMap<String, (ValueType, String)>,
    max_signals: usize,
}

impl Discovery {
    /// A discovery for `format`, as errors name it, holding at most `max_signals`.
    #[must_use]
    pub fn new(format: &'static str, max_signals: usize) -> Self {
        Self {
            format,
            known: HashMap::new(),
            max_signals,
        }
    }

    /// Records a signal a file declared, so its rows report nothing new.
    pub fn declare(&mut self, signal: &Signal) {
        let kind = ValueType::try_from(signal.value_type).unwrap_or(ValueType::Unspecified);
        let entry = (kind, signal.stream.clone());
        self.known.insert(signal.name.clone(), entry);
    }

    /// Checks one row, returning the [`Signal`] to create when this is the
    /// signal's first. The result copies what it needs.
    ///
    /// # Errors
    /// [`AdapterError::Malformed`] naming `row` for a bad signal name or a row
    /// disagreeing with an earlier one; [`AdapterError::LimitExceeded`] past
    /// `max_signals`.
    pub fn observe(
        &mut self,
        name: &str,
        kind: ValueType,
        stream: &str,
        row: u64,
    ) -> Result<Option<Signal>, AdapterError> {
        let bad = |detail: String| AdapterError::Malformed {
            format: self.format,
            row: Some(row),
            detail,
        };
        if let Some((known_kind, known_stream)) = self.known.get(name) {
            if *known_kind != kind {
                let (a, b) = (known_kind.as_str_name(), kind.as_str_name());
                return Err(bad(format!("{name} was {a}, this row says {b}")));
            }
            if known_stream != stream {
                return Err(bad(format!(
                    "{name} records into {known_stream}, this row says {stream}"
                )));
            }
            return Ok(None);
        }
        if entity_of(name).is_none() {
            return Err(bad(format!(
                "{name:?} is not entities/{{entity}}/signals/{{signal}}"
            )));
        }
        if !stream.starts_with("streams/") {
            return Err(bad(format!("{stream:?} is not streams/{{stream}}")));
        }
        if self.known.len() >= self.max_signals {
            return Err(AdapterError::LimitExceeded {
                limit: "max_signals",
                value: self.max_signals as u64,
            });
        }
        self.known
            .insert(name.to_owned(), (kind, stream.to_owned()));
        Ok(Some(Signal {
            name: name.to_owned(),
            value_type: kind as i32,
            stream: stream.to_owned(),
            ..Signal::default()
        }))
    }
}
