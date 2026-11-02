//! The stable byte form of `Value`: lossless, bounded, and strict.

use norn_core::{Error, Value, ValueKind};
use proptest::prelude::*;

fn any_value() -> impl Strategy<Value = Value> {
    prop_oneof![
        any::<bool>().prop_map(Value::Bool),
        any::<i32>().prop_map(Value::I32),
        any::<i64>().prop_map(Value::I64),
        any::<u32>().prop_map(Value::U32),
        any::<u64>().prop_map(Value::U64),
        any::<u32>().prop_map(|b| Value::F32(f32::from_bits(b))),
        any::<u64>().prop_map(|b| Value::F64(f64::from_bits(b))),
        ".{0,64}".prop_map(Value::String),
        proptest::collection::vec(any::<u8>(), 0..64).prop_map(Value::Bytes),
        any::<u32>().prop_map(Value::Enum),
    ]
}

// A test helper sits outside `#[test]`, so clippy's in-test allowance misses it.
#[allow(clippy::unwrap_used)]
fn encode(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode(&mut out).unwrap();
    out
}

proptest! {
    #[test]
    fn every_value_survives_encode_then_decode_bit_for_bit(v in any_value()) {
        let bytes = encode(&v);
        prop_assert_eq!(bytes.len(), v.encoded_len());
        let (back, used) = Value::decode(&bytes).unwrap();
        prop_assert_eq!(used, bytes.len());
        prop_assert_eq!(back.kind(), v.kind());
        prop_assert_eq!(encode(&back), bytes);
    }

    #[test]
    fn trailing_bytes_are_left_for_the_caller(v in any_value(), tail in proptest::collection::vec(any::<u8>(), 0..8)) {
        let mut bytes = encode(&v);
        let len = bytes.len();
        bytes.extend_from_slice(&tail);
        let (_, used) = Value::decode(&bytes).unwrap();
        prop_assert_eq!(used, len);
    }

    #[test]
    fn every_strict_prefix_is_refused_as_truncated(v in any_value()) {
        let bytes = encode(&v);
        for cut in 0..bytes.len() {
            let refused = matches!(Value::decode(&bytes[..cut]), Err(Error::Truncated { .. }));
            prop_assert!(refused, "prefix of {} bytes decoded", cut);
        }
    }

    #[test]
    fn arbitrary_input_never_panics(input in proptest::collection::vec(any::<u8>(), 0..32)) {
        let _ = Value::decode(&input);
    }
}

#[test]
fn tags_are_frozen_because_they_are_storage_format() {
    let tags: Vec<u8> = ValueKind::ALL.iter().map(|k| k.tag()).collect();
    assert_eq!(tags, [1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
}

#[test]
fn a_known_layout_decodes_to_the_expected_value() {
    assert_eq!(encode(&Value::I32(-2)), [0x02, 0xfe, 0xff, 0xff, 0xff]);
    assert_eq!(
        encode(&Value::String("hi".into())),
        [0x08, 2, 0, 0, 0, b'h', b'i']
    );
}

#[test]
fn an_unknown_tag_is_refused_rather_than_misread() {
    assert_eq!(
        Value::decode(&[0x00]),
        Err(Error::UnknownValueTag { tag: 0 })
    );
    assert_eq!(
        Value::decode(&[0x7f, 1, 2]),
        Err(Error::UnknownValueTag { tag: 0x7f })
    );
}

#[test]
fn a_boolean_byte_other_than_zero_or_one_is_malformed() {
    assert_eq!(
        Value::decode(&[0x01, 2]),
        Err(Error::MalformedValue { tag: 1 })
    );
}

#[test]
fn a_string_that_is_not_utf8_is_refused_with_its_offset() {
    let bytes = [0x08, 1, 0, 0, 0, 0xff];
    assert_eq!(Value::decode(&bytes), Err(Error::InvalidUtf8 { offset: 5 }));
}

#[test]
fn a_huge_length_prefix_allocates_nothing_and_is_truncated() {
    let bytes = [0x09, 0xff, 0xff, 0xff, 0xff, 1, 2];
    assert!(matches!(
        Value::decode(&bytes),
        Err(Error::Truncated { .. })
    ));
}
