//! The portable description of what samples belong to.

use std::collections::BTreeSet;

use norn_protocol::entity::Entity;
use norn_protocol::signal::Signal;
use norn_protocol::stream::Stream;

/// The streams, entities and signals of an export: enough to recreate them.
///
/// Only what the creator chose is carried: names, display names, annotations,
/// units, value types and each signal's stream. Times, etags and statistics are
/// the daemon's, so they never travel; nor does a stream's retention, which is
/// the target operator's decision (SPEC §43).
///
/// Bounded by [`crate::DEFAULT_MAX_SIGNALS`] and the daemon's metadata limits; it
/// is the one per-signal thing the adapters hold. Plain owned data.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Catalog {
    /// The streams, in name order.
    pub streams: Vec<Stream>,
    /// The entities, in name order.
    pub entities: Vec<Entity>,
    /// The signals, in name order.
    pub signals: Vec<Signal>,
}

impl Catalog {
    /// Whether any signal, entity or stream carries metadata beyond its identity,
    /// which a format that has no place for it would lose.
    #[must_use]
    pub fn has_metadata(&self) -> bool {
        self.streams
            .iter()
            .any(|s| !s.display_name.is_empty() || !s.annotations.is_empty())
            || self
                .entities
                .iter()
                .any(|e| !e.display_name.is_empty() || !e.annotations.is_empty())
            || self.signals.iter().any(|s| {
                !s.display_name.is_empty() || !s.annotations.is_empty() || !s.unit.is_empty()
            })
    }

    /// The names of the streams the signals record into, sorted.
    #[must_use]
    pub fn stream_names(&self) -> BTreeSet<&str> {
        self.signals.iter().map(|s| s.stream.as_str()).collect()
    }
}

/// `entities/a` of `entities/a/signals/b`; `None` if `signal` has any other shape.
/// The result borrows `signal`.
#[must_use]
pub fn entity_of(signal: &str) -> Option<&str> {
    let (entity, id) = signal.rsplit_once("/signals/")?;
    let bare = entity.strip_prefix("entities/")?;
    let valid = !bare.is_empty() && !bare.contains('/') && !id.is_empty() && !id.contains('/');
    valid.then_some(entity)
}

/// `(entity_id, signal_id)` of `entities/a/signals/b`. Borrows `signal`.
#[must_use]
pub fn entity_resource(signal: &str) -> Option<(&str, &str)> {
    let entity = entity_of(signal)?;
    let id = signal.rsplit_once("/signals/")?.1;
    Some((entity.strip_prefix("entities/")?, id))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_full_signal_name_has_an_entity() {
        assert_eq!(entity_of("entities/a/signals/b"), Some("entities/a"));
        assert_eq!(entity_resource("entities/a/signals/b"), Some(("a", "b")));
        for bad in [
            "a/b",
            "entities//signals/b",
            "entities/a/signals/",
            "x/a/signals/b",
        ] {
            assert_eq!(entity_of(bad), None, "{bad}");
        }
    }
}
