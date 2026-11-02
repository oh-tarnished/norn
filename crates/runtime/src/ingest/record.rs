//! A batch's byte form inside the journal.
//!
//! ```text
//! count u32, then per sample: signal u64 | time i64 | quality u16 | sequence u64
//!                             | Value::encode bytes
//! ```
//!
//! All little-endian. The frame around it carries the CRC, so one frame is one
//! whole batch.

use norn_core::{Quality, Sample, Sequence, SignalId, Timestamp, Value};

const FIXED: usize = 26;

/// Encodes `samples`. Copies; allocates once when values are fixed width.
pub(crate) fn encode(samples: &[Sample], out: &mut Vec<u8>) -> Result<(), norn_core::Error> {
    out.reserve(4 + samples.len() * (FIXED + 9));
    let count = u32::try_from(samples.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&count.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.signal.get().to_le_bytes());
        out.extend_from_slice(&s.timestamp.as_nanos().to_le_bytes());
        out.extend_from_slice(&s.quality.code().to_le_bytes());
        out.extend_from_slice(&s.sequence.get().to_le_bytes());
        s.value.encode(out)?;
    }
    Ok(())
}

/// Decodes a record whose frame passed its CRC, or `None` if it is not one.
/// Copies: the samples own their payloads and do not borrow `bytes`.
pub(crate) fn decode(bytes: &[u8]) -> Option<Vec<Sample>> {
    let (head, mut rest) = bytes.split_at_checked(4)?;
    let count = u32::from_le_bytes(head.try_into().ok()?);
    let mut out = Vec::with_capacity((count as usize).min(rest.len() / FIXED));
    for _ in 0..count {
        let (fixed, tail) = rest.split_at_checked(FIXED)?;
        let signal = u64::from_le_bytes(fixed.get(..8)?.try_into().ok()?);
        let time = i64::from_le_bytes(fixed.get(8..16)?.try_into().ok()?);
        let quality = u16::from_le_bytes(fixed.get(16..18)?.try_into().ok()?);
        let sequence = u64::from_le_bytes(fixed.get(18..26)?.try_into().ok()?);
        let (value, used) = Value::decode(tail).ok()?;
        rest = tail.get(used..)?;
        out.push(Sample::new(
            Timestamp::from_nanos(time),
            SignalId::new(signal),
            value,
            Quality::from_code(quality).ok()?,
            Sequence::new(sequence),
        ));
    }
    rest.is_empty().then_some(out)
}
