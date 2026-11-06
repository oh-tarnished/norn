//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! [`Sink::declare`] and [`Sink::push`]: checked whole before anything changes,
//! then queued under the one lock, for the run to carry out.

use super::Sink;
use crate::catalog::Catalog;
use crate::error::SinkError;
use crate::health::Notice;
use crate::ids;
use crate::reading::Reading;
use crate::shared::State;
use std::collections::{HashMap, HashSet};
use std::time::Instant;

/// What a [`Sink::declare`] queued: what was new or had changed. Plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Declared {
    /// Entities queued.
    pub entities: usize,
    /// Signals queued.
    pub signals: usize,
}

/// What became of a [`Sink::push`]. Plain data.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Pushed {
    /// Readings now held, for Norn.
    pub queued: usize,
    /// Readings given up for want of room, by this push: its own, or older
    /// ones it displaced. Counted in [`Counts`](crate::Counts), and told as a
    /// [`Notice::Dropped`].
    pub dropped: usize,
}

/// Every reason `catalog` cannot be declared, the first found.
fn check(catalog: &Catalog, state: &State, max_signals: usize) -> Result<(), SinkError> {
    let mut entities = HashSet::new();
    for entity in &catalog.entities {
        if !ids::is_valid(&entity.id) {
            return Err(SinkError::InvalidId {
                field: "entity_id",
                id: entity.id.clone(),
            });
        }
        entities.insert(entity.id.as_str());
    }
    let mut fresh = HashMap::new();
    for signal in &catalog.signals {
        if !ids::is_valid(&signal.id) {
            return Err(SinkError::InvalidId {
                field: "signal_id",
                id: signal.id.clone(),
            });
        }
        let name = signal.name();
        let parent = ids::entity_name(&signal.entity);
        if !entities.contains(signal.entity.as_str()) && !state.entities.contains_key(&parent) {
            return Err(SinkError::UnknownEntity { signal: name });
        }
        let known = state.signals.get(&name).map(|s| s.value_type);
        let declared = known.or_else(|| fresh.get(&name).copied());
        match declared {
            Some(declared) if declared != signal.value_type => {
                return Err(SinkError::TypeChanged {
                    signal: name,
                    declared,
                    now: signal.value_type,
                });
            }
            Some(_) => {}
            None => {
                fresh.insert(name, signal.value_type);
            }
        }
    }
    if state.signals.len() + fresh.len() > max_signals {
        return Err(SinkError::TooManySignals { limit: max_signals });
    }
    Ok(())
}

impl Sink {
    /// Declares `catalog`. Returns at once: the run creates what is new, and a
    /// reading of a signal not yet made waits for that signal alone. What is
    /// declared again unchanged costs nothing; what has changed is updated.
    ///
    /// # Errors
    ///
    /// [`SinkError::InvalidId`], [`SinkError::UnknownEntity`],
    /// [`SinkError::TypeChanged`], [`SinkError::TooManySignals`] or
    /// [`SinkError::Closed`]. Nothing of the catalog is then declared.
    pub fn declare(&self, catalog: &Catalog) -> Result<Declared, SinkError> {
        if self.shared.is_closing() {
            return Err(SinkError::Closed);
        }
        let stream = ids::stream_name(&self.shared.stream);
        let mut declared = Declared::default();
        {
            let mut state = self.shared.state();
            check(catalog, &state, self.shared.options.max_signals)?;
            let state = &mut *state;
            for entity in &catalog.entities {
                let name = ids::entity_name(&entity.id);
                if state.entities.get(&name) != Some(entity) {
                    state.work.push(entity.resource());
                    state.entities.insert(name, entity.clone());
                    declared.entities += 1;
                }
            }
            for signal in &catalog.signals {
                let name = signal.name();
                if state.signals.get(&name) != Some(signal) {
                    if !state.made.contains(&name) {
                        state.flow.block(&name);
                    }
                    state.work.push(signal.resource(&stream));
                    state.signals.insert(name, signal.clone());
                    declared.signals += 1;
                }
            }
        }
        self.shared.wake.notify_one();
        Ok(declared)
    }

    /// Hands over `readings`, in their order, and returns at once. Past the
    /// byte bound or a signal's share, readings are given up and counted,
    /// because the bound is on memory and so cannot be a wait.
    ///
    /// # Errors
    ///
    /// [`SinkError::Undeclared`], [`SinkError::WrongType`] or
    /// [`SinkError::Closed`]. None of `readings` is then taken.
    pub fn push(&self, readings: impl IntoIterator<Item = Reading>) -> Result<Pushed, SinkError> {
        if self.shared.is_closing() {
            return Err(SinkError::Closed);
        }
        let readings: Vec<Reading> = readings.into_iter().collect();
        let queued = readings.len();
        let (gone, told) = {
            let mut state = self.shared.state();
            for reading in &readings {
                let Some(signal) = state.signals.get(reading.signal()) else {
                    return Err(SinkError::Undeclared {
                        signal: reading.signal().to_owned(),
                    });
                };
                let got = reading.value_type();
                if signal.value_type != got {
                    return Err(SinkError::WrongType {
                        signal: reading.signal().to_owned(),
                        declared: signal.value_type,
                        got,
                    });
                }
            }
            let state = &mut *state;
            let samples = readings.into_iter().map(|reading| {
                let sequence = state.sequence;
                state.sequence += 1;
                reading.into_sample(sequence)
            });
            let samples = samples.collect();
            let before = state.flow.counts();
            let gone = state.flow.offer(samples);
            let told = (gone > 0).then(|| {
                state.dropped_at = Some(Instant::now());
                let after = state.flow.counts();
                Notice::Dropped {
                    overflowed: after.overflowed - before.overflowed,
                    gated: after.gated - before.gated,
                    held_bytes: state.flow.held_bytes(),
                }
            });
            (gone, told)
        };
        if let Some(notice) = told {
            self.shared.tell(&notice);
        }
        self.shared.wake.notify_one();
        Ok(Pushed {
            queued: queued.saturating_sub(gone),
            dropped: gone,
        })
    }
}
