//! The stable byte form of a [`Value`].
//!
//! Layout: one tag byte ([`ValueKind::tag`]), then the payload, little-endian.
//! Scalars are fixed width (bool 1, 32-bit 4, 64-bit 8; floats as their IEEE
//! bits). `String` and `Bytes` are a `u32` byte length followed by the bytes.
//!
//! This is a self-describing interchange form for single values (tests,
//! metadata, wire framing); it is not the columnar storage encoding, which the
//! codec layer chooses per block. It is versioned by its tags: a new kind adds
//! a tag, and no existing tag ever changes its layout.

use super::{Value, ValueKind};
use crate::error::{Error, Limit, Result};

impl Value {
    /// Appends this value's byte form to `out`.
    ///
    /// Performance: one tag byte plus the payload; reserves exactly once.
    ///
    /// # Errors
    ///
    /// [`Error::LimitExceeded`] with [`Limit::ValuePayloadBytes`] when a
    /// string or bytes payload exceeds `u32::MAX` bytes; `out` is unchanged.
    pub fn encode(&self, out: &mut Vec<u8>) -> Result<()> {
        let tag = self.kind().tag();
        match self {
            Self::Bool(v) => put(out, tag, &[u8::from(*v)]),
            Self::I32(v) => put(out, tag, &v.to_le_bytes()),
            Self::I64(v) => put(out, tag, &v.to_le_bytes()),
            Self::U32(v) | Self::Enum(v) => put(out, tag, &v.to_le_bytes()),
            Self::U64(v) => put(out, tag, &v.to_le_bytes()),
            Self::F32(v) => put(out, tag, &v.to_bits().to_le_bytes()),
            Self::F64(v) => put(out, tag, &v.to_bits().to_le_bytes()),
            Self::String(s) => put_len_prefixed(out, tag, s.as_bytes())?,
            Self::Bytes(b) => put_len_prefixed(out, tag, b)?,
        }
        Ok(())
    }

    /// Decodes one value from the front of `input`, returning it and the
    /// number of bytes consumed. Trailing bytes are left for the caller.
    ///
    /// Copies: the returned value owns its payload and does not borrow
    /// `input`. Memory is bounded by `input`'s length, because a length prefix
    /// is checked against the bytes actually present before anything is
    /// allocated.
    ///
    /// # Errors
    ///
    /// - [`Error::Truncated`] when `input` ends early.
    /// - [`Error::UnknownValueTag`] for a tag this build does not know.
    /// - [`Error::MalformedValue`] for a boolean byte other than 0 or 1.
    /// - [`Error::InvalidUtf8`] for a string payload that is not UTF-8.
    pub fn decode(input: &[u8]) -> Result<(Self, usize)> {
        let (&tag, rest) = input.split_first().ok_or(Error::Truncated {
            needed: 1,
            available: 0,
        })?;
        let kind = ValueKind::from_tag(tag).ok_or(Error::UnknownValueTag { tag })?;
        let value = match kind {
            ValueKind::Bool => match fixed::<1>(input)? {
                [0] => Self::Bool(false),
                [1] => Self::Bool(true),
                _ => return Err(Error::MalformedValue { tag }),
            },
            ValueKind::I32 => Self::I32(i32::from_le_bytes(fixed(input)?)),
            ValueKind::I64 => Self::I64(i64::from_le_bytes(fixed(input)?)),
            ValueKind::U32 => Self::U32(u32::from_le_bytes(fixed(input)?)),
            ValueKind::U64 => Self::U64(u64::from_le_bytes(fixed(input)?)),
            ValueKind::F32 => Self::F32(f32::from_bits(u32::from_le_bytes(fixed(input)?))),
            ValueKind::F64 => Self::F64(f64::from_bits(u64::from_le_bytes(fixed(input)?))),
            ValueKind::Enum => Self::Enum(u32::from_le_bytes(fixed(input)?)),
            ValueKind::String => {
                let bytes = prefixed(input, rest)?;
                let text =
                    std::str::from_utf8(bytes).map_err(|_| Error::InvalidUtf8 { offset: 5 })?;
                Self::String(text.to_owned())
            }
            ValueKind::Bytes => Self::Bytes(prefixed(input, rest)?.to_vec()),
        };
        let consumed = value.encoded_len();
        Ok((value, consumed))
    }

    /// The length of this value's byte form.
    #[must_use]
    pub fn encoded_len(&self) -> usize {
        1 + match self {
            Self::Bool(_) => 1,
            Self::I32(_) | Self::U32(_) | Self::F32(_) | Self::Enum(_) => 4,
            Self::I64(_) | Self::U64(_) | Self::F64(_) => 8,
            Self::String(s) => 4 + s.len(),
            Self::Bytes(b) => 4 + b.len(),
        }
    }
}

fn put(out: &mut Vec<u8>, tag: u8, payload: &[u8]) {
    out.reserve(1 + payload.len());
    out.push(tag);
    out.extend_from_slice(payload);
}

fn put_len_prefixed(out: &mut Vec<u8>, tag: u8, payload: &[u8]) -> Result<()> {
    let len = u32::try_from(payload.len()).map_err(|_| Error::LimitExceeded {
        limit: Limit::ValuePayloadBytes,
        max: usize::try_from(u32::MAX).unwrap_or(usize::MAX),
        actual: payload.len(),
    })?;
    out.reserve(5 + payload.len());
    out.push(tag);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(payload);
    Ok(())
}

/// The `N` payload bytes after the tag.
fn fixed<const N: usize>(input: &[u8]) -> Result<[u8; N]> {
    input
        .get(1..1 + N)
        .and_then(|s| <[u8; N]>::try_from(s).ok())
        .ok_or(Error::Truncated {
            needed: 1 + N,
            available: input.len(),
        })
}

/// The length-prefixed payload after the tag, borrowed from `input`.
fn prefixed<'a>(input: &'a [u8], rest: &'a [u8]) -> Result<&'a [u8]> {
    let len = u32::from_le_bytes(fixed::<4>(input)?);
    let len = usize::try_from(len).map_err(|_| Error::Overflow)?;
    let needed = 5usize.checked_add(len).ok_or(Error::Overflow)?;
    rest.get(4..needed - 1).ok_or(Error::Truncated {
        needed,
        available: input.len(),
    })
}
