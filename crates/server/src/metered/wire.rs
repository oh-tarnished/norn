//! Reading a protobuf message's top level without decoding it, so a batch's
//! sample count is known before prost allocates for it.

use bytes::{Bytes, BytesMut};

use super::reader::PREFIX;

/// A protobuf varint at the front of `b`, with its length.
fn varint(b: &[u8]) -> Option<(u64, usize)> {
    let mut value = 0u64;
    for (i, byte) in b.iter().take(10).enumerate() {
        value |= u64::from(byte & 0x7f) << (7 * i);
        if byte & 0x80 == 0 {
            return Some((value, i + 1));
        }
    }
    None
}

/// The top-level fields of a message: `(field number, whole field's range)`.
/// Stops at anything malformed, for prost to refuse. Borrows `b`, allocates
/// nothing, and costs one step per field.
fn fields(b: &[u8]) -> impl Iterator<Item = (u64, std::ops::Range<usize>)> + '_ {
    let mut at = 0usize;
    std::iter::from_fn(move || {
        let (tag, n) = varint(b.get(at..)?)?;
        let body = at + n;
        let end = match tag & 7 {
            0 => body + varint(b.get(body..)?)?.1,
            1 => body + 8,
            2 => {
                let (len, m) = varint(b.get(body..)?)?;
                body.checked_add(m)?
                    .checked_add(usize::try_from(len).ok()?)?
            }
            5 => body + 4,
            _ => return None,
        };
        let range = at..end;
        (end <= b.len()).then(|| {
            at = end;
            (tag >> 3, range)
        })
    })
}

/// Field 3 of `AppendSamplesRequest` is `samples`.
const SAMPLES_FIELD: u64 = 3;

pub(super) fn count_samples(payload: &[u8]) -> u64 {
    fields(payload).filter(|(f, _)| *f == SAMPLES_FIELD).count() as u64
}

/// The same message without its samples, prefixed: what remains is enough for
/// the handler to reject the batch by its `batch_id`. Copies.
pub(super) fn strip_samples(payload: &[u8]) -> Bytes {
    let mut kept = BytesMut::with_capacity(64);
    kept.extend_from_slice(&[0u8; PREFIX]);
    for (field, range) in fields(payload) {
        if field != SAMPLES_FIELD
            && let Some(bytes) = payload.get(range)
        {
            kept.extend_from_slice(bytes);
        }
    }
    let length = u32::try_from(kept.len() - PREFIX).unwrap_or(0);
    if let Some(slot) = kept.get_mut(1..PREFIX) {
        slot.copy_from_slice(&length.to_be_bytes());
    }
    kept.freeze()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message() -> Vec<u8> {
        // batch_id = 7 (field 2), two empty samples (field 3), ack_level = 4.
        vec![0x10, 7, 0x1a, 0, 0x1a, 0, 0x20, 4]
    }

    #[test]
    fn samples_are_counted_without_decoding() {
        assert_eq!(count_samples(&message()), 2);
    }

    #[test]
    fn stripping_keeps_the_other_fields_and_fixes_the_prefix() {
        let kept = strip_samples(&message());
        assert_eq!(&kept[..], &[0, 0, 0, 0, 4, 0x10, 7, 0x20, 4]);
    }

    #[test]
    fn a_truncated_message_counts_what_is_whole_and_stops() {
        assert_eq!(count_samples(&[0x1a, 0, 0x1a, 9]), 1);
        assert_eq!(count_samples(&[0xff]), 0);
    }
}
