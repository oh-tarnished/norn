//! Opaque, bounded string metadata.
//!
//! This is where domain knowledge lives without the core learning it: a BACnet
//! adapter attaches `protocol=BACnet` and `object_type=analogInput`, a Modbus
//! adapter attaches its register map, and the core stores both as strings it
//! never interprets (SPEC §4, §11).

use std::collections::BTreeMap;

use crate::error::{Error, Limit, Result};

/// The bounds one [`Metadata`] map enforces.
///
/// SPEC §41: a client must not exhaust memory by attaching unlimited
/// metadata, so every map carries its limits.
///
/// `Default`: 64 entries, 128-byte keys, 1 KiB values. These are picked
/// defaults, not measured ones: generous for any adapter mapping seen so far,
/// and small enough that 100,000 signals' metadata stays in tens of MiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MetadataLimits {
    /// The most entries one map holds.
    pub max_entries: usize,
    /// The longest key, in UTF-8 bytes.
    pub max_key_bytes: usize,
    /// The longest value, in UTF-8 bytes.
    pub max_value_bytes: usize,
}

impl Default for MetadataLimits {
    fn default() -> Self {
        Self {
            max_entries: 64,
            max_key_bytes: 128,
            max_value_bytes: 1024,
        }
    }
}

/// An ordered string-to-string map with enforced [`MetadataLimits`].
///
/// Ordered (a `BTreeMap`) so iteration, encoding and comparison are
/// deterministic, which replay and tests depend on.
///
/// Concurrency: owned data; `Send + Sync`; mutation needs `&mut`.
/// Performance: `O(log n)` insert and lookup; `n` is bounded by the limits.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Metadata {
    entries: BTreeMap<String, String>,
    limits: MetadataLimits,
}

impl Metadata {
    /// An empty map enforcing `limits`.
    #[must_use]
    pub fn with_limits(limits: MetadataLimits) -> Self {
        Self {
            entries: BTreeMap::new(),
            limits,
        }
    }

    /// Inserts or replaces `key`, returning the previous value.
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`] naming the key, value or entry-count limit.
    /// On error the map is unchanged.
    pub fn insert(
        &mut self,
        key: impl Into<String>,
        value: impl Into<String>,
    ) -> Result<Option<String>> {
        let (key, value) = (key.into(), value.into());
        check(
            Limit::MetadataKeyBytes,
            self.limits.max_key_bytes,
            key.len(),
        )?;
        check(
            Limit::MetadataValueBytes,
            self.limits.max_value_bytes,
            value.len(),
        )?;
        if !self.entries.contains_key(&key) {
            let after = self.entries.len().saturating_add(1);
            check(Limit::MetadataEntries, self.limits.max_entries, after)?;
        }
        Ok(self.entries.insert(key, value))
    }

    /// The value for `key`, if present.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries.get(key).map(String::as_str)
    }

    /// Removes `key`, returning its value.
    pub fn remove(&mut self, key: &str) -> Option<String> {
        self.entries.remove(key)
    }

    /// Entries in key order.
    pub fn iter(&self) -> impl Iterator<Item = (&str, &str)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v.as_str()))
    }

    /// The number of entries.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the map is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The limits this map enforces.
    #[must_use]
    pub fn limits(&self) -> MetadataLimits {
        self.limits
    }
}

fn check(limit: Limit, max: usize, actual: usize) -> Result<()> {
    if actual > max {
        return Err(Error::LimitExceeded { limit, max, actual });
    }
    Ok(())
}
