//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Observed values.
//!
//! [`Value`] is the closed-today, open-tomorrow set of things a sample can
//! carry. Every kind has a stable one-byte tag ([`ValueKind::tag`]) which is
//! part of the storage format: a new kind gets a new tag and existing tags
//! never change meaning, so data written today decodes identically forever,
//! and data written by a newer Norn is refused by an older one with
//! [`crate::Error::UnknownValueTag`] instead of being misread (SPEC §40).

mod codec;

/// One observed value.
///
/// `#[non_exhaustive]` so a new kind is not a breaking change; callers match
/// with a wildcard arm.
///
/// `Enum` holds an ordinal only. What the ordinal means (`0 = off`,
/// `3 = auto`) is per-signal and lives in the signal's metadata, so the core
/// never learns any domain's vocabulary.
///
/// Equality is Rust's: `F64(NaN) != F64(NaN)`. Compare the byte form
/// ([`Value::encode`]) when bit-exact equality is wanted.
///
/// Concurrency: plain owned data; `Send + Sync`.
/// Performance: scalars are inline; `String` and `Bytes` own one heap
/// allocation each.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Value {
    /// A boolean.
    Bool(bool),
    /// A signed 32-bit integer.
    I32(i32),
    /// A signed 64-bit integer.
    I64(i64),
    /// An unsigned 32-bit integer.
    U32(u32),
    /// An unsigned 64-bit integer.
    U64(u64),
    /// An IEEE 754 single; every bit pattern, NaN payloads included, is kept.
    F32(f32),
    /// An IEEE 754 double; every bit pattern, NaN payloads included, is kept.
    F64(f64),
    /// UTF-8 text.
    String(String),
    /// Opaque bytes.
    Bytes(Vec<u8>),
    /// An enumeration ordinal; its labels live in signal metadata.
    Enum(u32),
}

/// The kind of a [`Value`], without its payload.
///
/// A signal declares one, so storage can pick an encoding per kind (SPEC §14).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ValueKind {
    /// [`Value::Bool`].
    Bool,
    /// [`Value::I32`].
    I32,
    /// [`Value::I64`].
    I64,
    /// [`Value::U32`].
    U32,
    /// [`Value::U64`].
    U64,
    /// [`Value::F32`].
    F32,
    /// [`Value::F64`].
    F64,
    /// [`Value::String`].
    String,
    /// [`Value::Bytes`].
    Bytes,
    /// [`Value::Enum`].
    Enum,
}

impl ValueKind {
    /// Every kind, in tag order.
    pub const ALL: [Self; 10] = [
        Self::Bool,
        Self::I32,
        Self::I64,
        Self::U32,
        Self::U64,
        Self::F32,
        Self::F64,
        Self::String,
        Self::Bytes,
        Self::Enum,
    ];

    /// The stable storage tag. Tag `0` is reserved and never valid, so a
    /// zero-filled region is never mistaken for a value.
    #[must_use]
    pub const fn tag(self) -> u8 {
        match self {
            Self::Bool => 0x01,
            Self::I32 => 0x02,
            Self::I64 => 0x03,
            Self::U32 => 0x04,
            Self::U64 => 0x05,
            Self::F32 => 0x06,
            Self::F64 => 0x07,
            Self::String => 0x08,
            Self::Bytes => 0x09,
            Self::Enum => 0x0a,
        }
    }

    /// The kind with this tag, or `None` for a tag this build does not know.
    #[must_use]
    pub fn from_tag(tag: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.tag() == tag)
    }
}

impl Value {
    /// This value's kind.
    #[must_use]
    pub const fn kind(&self) -> ValueKind {
        match self {
            Self::Bool(_) => ValueKind::Bool,
            Self::I32(_) => ValueKind::I32,
            Self::I64(_) => ValueKind::I64,
            Self::U32(_) => ValueKind::U32,
            Self::U64(_) => ValueKind::U64,
            Self::F32(_) => ValueKind::F32,
            Self::F64(_) => ValueKind::F64,
            Self::String(_) => ValueKind::String,
            Self::Bytes(_) => ValueKind::Bytes,
            Self::Enum(_) => ValueKind::Enum,
        }
    }
}
