//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which signals one read covers.

use std::sync::Arc;

use norn_core::SignalId;
use norn_index::SignalFilter;

use crate::spec::Target;

/// Some of a query's targets, ready for planning.
#[derive(Debug, Clone)]
pub(crate) struct Selection {
    pub(crate) targets: Vec<(u32, Arc<Target>)>,
    ids: Vec<(SignalId, u32)>,
    pub(crate) filter: SignalFilter,
}

impl Selection {
    pub(crate) fn new(targets: Vec<(u32, Arc<Target>)>) -> Self {
        let mut ids: Vec<_> = targets.iter().map(|(i, t)| (t.signal, *i)).collect();
        ids.sort_unstable();
        let filter = SignalFilter::many(ids.iter().map(|e| e.0));
        Self {
            targets,
            ids,
            filter,
        }
    }

    pub(crate) fn target_of(&self, signal: SignalId) -> Option<u32> {
        let at = self.ids.binary_search_by_key(&signal, |e| e.0).ok()?;
        self.ids.get(at).map(|e| e.1)
    }
}
