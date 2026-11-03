//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which signals a filter selects: for ListSignals, and for a query.

use norn_metadata::{Registry, SignalRecord};
use norn_protocol::common::ValueType;
use tonic::Status;

use super::query::{Clause, matches, parse_filter};
use super::{metadata_status, names};
use crate::convert::kind_to_wire;

/// The fields of the signal filter.
pub(super) const FILTERABLE: [&str; 5] = ["display_name", "id", "unit", "stream", "value_type"];

fn value_type_name(number: i32) -> String {
    ValueType::try_from(number)
        .map(|t| t.as_str_name().to_owned())
        .unwrap_or_default()
}

/// Whether signal `s` satisfies `clauses`.
pub(super) fn signal_matches(registry: &Registry, clauses: &[Clause], s: &SignalRecord) -> bool {
    matches(clauses, |field| match field {
        "display_name" => Some(s.display_name.clone()),
        "id" => Some(s.descriptor.name.as_str().to_owned()),
        "unit" => Some(s.unit.clone()),
        "stream" => {
            let name = registry.stream_name(s.stream)?;
            Some(format!("streams/{name}"))
        }
        "value_type" => Some(value_type_name(kind_to_wire(s.descriptor.kind))),
        other => other
            .strip_prefix("annotations.")
            .and_then(|k| s.descriptor.metadata.get(k))
            .map(str::to_owned),
    })
}

/// Every signal under `parent` matching `filter`, as a query selects them.
/// Walks pages of the registry, so its lock is never held for all of them.
pub(crate) fn select_signals(
    registry: &Registry,
    parent: &str,
    filter: &str,
) -> Result<Vec<(String, SignalRecord)>, Status> {
    const PAGE: usize = 1_000;
    let clauses = parse_filter(filter, &FILTERABLE)?;
    let entity = (parent != "entities/-")
        .then(|| names::entity(parent))
        .transpose()?;
    if let Some(entity) = entity {
        registry.entity(entity).map_err(|e| metadata_status(&e))?;
    }
    let mut out: Vec<(String, SignalRecord)> = Vec::new();
    loop {
        let after = out
            .last()
            .map(|(e, s)| (e.clone(), s.descriptor.name.as_str().to_owned()));
        let page = registry.list_signals(entity, after, PAGE, |s| {
            signal_matches(registry, &clauses, s)
        });
        let done = page.len() < PAGE;
        out.extend(page);
        if done {
            return Ok(out);
        }
    }
}
