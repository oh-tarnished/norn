//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Whether what Norn holds after a batch is what was declared: a batch create
//! returns an existing resource as stored, display name, unit and annotations
//! left alone, so a change since a past run is found here and updated later.

use crate::provision::Resource;
use norn_protocol::entity::Entity;
use norn_protocol::signal::Signal;
use std::collections::HashMap;

/// What Norn holds after the batch, by full name.
pub(crate) enum Stored {
    Entities(HashMap<String, Entity>),
    Signals(HashMap<String, Signal>),
}

/// Whether Norn's copy of `resource` differs from it in what an update sets.
pub(crate) fn stale(resource: &Resource, stored: &Stored) -> bool {
    match (resource, stored) {
        (Resource::Entity(e), Stored::Entities(held)) => held
            .get(&e.name)
            .is_none_or(|n| n.display_name != e.display_name || n.annotations != e.annotations),
        (Resource::Signal { signal: s, .. }, Stored::Signals(held)) => {
            held.get(&s.name).is_none_or(|n| {
                n.display_name != s.display_name
                    || n.unit != s.unit
                    || n.annotations != s.annotations
            })
        }
        _ => true,
    }
}
