//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Compact identifiers for entities, signals and streams.
//!
//! Each is a distinct `u64` newtype so an [`EntityId`] can never be passed
//! where a [`SignalId`] is expected. A `u64` rather than a `u32`: SPEC §15
//! sizes one deployment at 16,000 signals, but identifiers are never reused, so a
//! long-lived database must not run out; storage is free to dictionary-encode
//! them more tightly. Who allocates identifiers is the runtime's decision, not
//! the core's.
//!
//! Concurrency: `Copy` plain data; `Send + Sync`.
//! Performance: eight bytes, no allocation, hashing and ordering are integer
//! operations.

macro_rules! id {
    ($(#[$doc:meta])* $name:ident) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
        pub struct $name(u64);

        impl $name {
            /// Wraps a raw identifier. Every `u64` is valid; zero carries no
            /// special meaning in the core.
            #[must_use]
            pub const fn new(raw: u64) -> Self {
                Self(raw)
            }

            /// The raw identifier, for storage and wire encodings.
            #[must_use]
            pub const fn get(self) -> u64 {
                self.0
            }
        }

        impl From<u64> for $name {
            fn from(raw: u64) -> Self {
                Self(raw)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id! {
    /// Identifies an [`crate::EntityDescriptor`]: a thing that exists.
    EntityId
}

id! {
    /// Identifies a [`crate::SignalDescriptor`]: an observable property of an
    /// entity.
    SignalId
}

id! {
    /// Identifies a [`crate::StreamDescriptor`]: a logical sequence of
    /// temporal observations.
    StreamId
}
