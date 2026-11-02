//! A column of values of one kind, stored natively rather than as `Value`s.

use norn_core::{Value, ValueKind};

use crate::error::{Result, StorageError};

/// The values of one chunk, one vector per [`ValueKind`].
///
/// Storing `Vec<f64>` rather than `Vec<Value>` is what lets each column go
/// straight to its codec, and keeps a decoded chunk at its natural width
/// (8 bytes per `f64` instead of a 32-byte `Value`).
///
/// Concurrency: owned plain data; `Send + Sync`.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub enum Column {
    /// [`Value::Bool`]s, stored with the boolean codec.
    Bool(Vec<bool>),
    /// [`Value::I32`]s, widened to `i64` for the integer codec.
    I32(Vec<i32>),
    /// [`Value::I64`]s, stored with the integer codec.
    I64(Vec<i64>),
    /// [`Value::U32`]s, widened to `i64` for the integer codec.
    U32(Vec<u32>),
    /// [`Value::U64`]s, bit-reinterpreted as `i64` for the integer codec.
    U64(Vec<u64>),
    /// [`Value::F32`]s, stored with the `f32` codec (bit-exact).
    F32(Vec<f32>),
    /// [`Value::F64`]s, stored with the `f64` codec (bit-exact).
    F64(Vec<f64>),
    /// [`Value::String`]s, stored with the bytes codec.
    String(Vec<String>),
    /// [`Value::Bytes`], stored with the bytes codec.
    Bytes(Vec<Vec<u8>>),
    /// [`Value::Enum`] ordinals, stored with the enumeration codec.
    Enum(Vec<u32>),
}

impl Column {
    /// An empty column of `kind`.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for a kind this build cannot store
    /// (one added to `norn_core` after this crate).
    pub fn new(kind: ValueKind) -> Result<Self> {
        Ok(match kind {
            ValueKind::Bool => Self::Bool(Vec::new()),
            ValueKind::I32 => Self::I32(Vec::new()),
            ValueKind::I64 => Self::I64(Vec::new()),
            ValueKind::U32 => Self::U32(Vec::new()),
            ValueKind::U64 => Self::U64(Vec::new()),
            ValueKind::F32 => Self::F32(Vec::new()),
            ValueKind::F64 => Self::F64(Vec::new()),
            ValueKind::String => Self::String(Vec::new()),
            ValueKind::Bytes => Self::Bytes(Vec::new()),
            ValueKind::Enum => Self::Enum(Vec::new()),
            _ => return Err(StorageError::InvalidChunk("value kind not storable")),
        })
    }

    /// The kind of every value in the column.
    #[must_use]
    pub fn kind(&self) -> ValueKind {
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

    /// Number of values.
    #[must_use]
    pub fn len(&self) -> usize {
        match self {
            Self::Bool(v) => v.len(),
            Self::I32(v) => v.len(),
            Self::I64(v) => v.len(),
            Self::U32(v) | Self::Enum(v) => v.len(),
            Self::U64(v) => v.len(),
            Self::F32(v) => v.len(),
            Self::F64(v) => v.len(),
            Self::String(v) => v.len(),
            Self::Bytes(v) => v.len(),
        }
    }

    /// Whether the column is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Appends `value`, which must be of the column's kind.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for a value of another kind; the
    /// column is unchanged.
    pub fn push(&mut self, value: &Value) -> Result<()> {
        match (self, value) {
            (Self::Bool(c), Value::Bool(v)) => c.push(*v),
            (Self::I32(c), Value::I32(v)) => c.push(*v),
            (Self::I64(c), Value::I64(v)) => c.push(*v),
            (Self::U32(c), Value::U32(v)) => c.push(*v),
            (Self::U64(c), Value::U64(v)) => c.push(*v),
            (Self::F32(c), Value::F32(v)) => c.push(*v),
            (Self::F64(c), Value::F64(v)) => c.push(*v),
            (Self::String(c), Value::String(v)) => c.push(v.clone()),
            (Self::Bytes(c), Value::Bytes(v)) => c.push(v.clone()),
            (Self::Enum(c), Value::Enum(v)) => c.push(*v),
            _ => {
                return Err(StorageError::InvalidChunk(
                    "value kind differs from the chunk's",
                ));
            }
        }
        Ok(())
    }

    /// The value at `i`, copied out, or `None` past the end.
    #[must_use]
    pub fn get(&self, i: usize) -> Option<Value> {
        Some(match self {
            Self::Bool(c) => Value::Bool(*c.get(i)?),
            Self::I32(c) => Value::I32(*c.get(i)?),
            Self::I64(c) => Value::I64(*c.get(i)?),
            Self::U32(c) => Value::U32(*c.get(i)?),
            Self::U64(c) => Value::U64(*c.get(i)?),
            Self::F32(c) => Value::F32(*c.get(i)?),
            Self::F64(c) => Value::F64(*c.get(i)?),
            Self::String(c) => Value::String(c.get(i)?.clone()),
            Self::Bytes(c) => Value::Bytes(c.get(i)?.clone()),
            Self::Enum(c) => Value::Enum(*c.get(i)?),
        })
    }

    /// Appends every value of `other`, which must be of the same kind.
    ///
    /// # Errors
    /// [`StorageError::InvalidChunk`] for a column of another kind.
    pub fn extend(&mut self, other: &Self) -> Result<()> {
        match (self, other) {
            (Self::Bool(a), Self::Bool(b)) => a.extend_from_slice(b),
            (Self::I32(a), Self::I32(b)) => a.extend_from_slice(b),
            (Self::I64(a), Self::I64(b)) => a.extend_from_slice(b),
            (Self::U32(a), Self::U32(b)) | (Self::Enum(a), Self::Enum(b)) => a.extend_from_slice(b),
            (Self::U64(a), Self::U64(b)) => a.extend_from_slice(b),
            (Self::F32(a), Self::F32(b)) => a.extend_from_slice(b),
            (Self::F64(a), Self::F64(b)) => a.extend_from_slice(b),
            (Self::String(a), Self::String(b)) => a.extend_from_slice(b),
            (Self::Bytes(a), Self::Bytes(b)) => a.extend_from_slice(b),
            _ => {
                return Err(StorageError::InvalidChunk(
                    "value kind differs from the chunk's",
                ));
            }
        }
        Ok(())
    }
}
