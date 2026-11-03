//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! AIP-141 forbids unsigned wire types, so an unsigned 64-bit value travels as
//! the `int64` with the same bits (docs/reference/protocol.md, "Unsigned values").

use norn_protocol::common::{Value, value::Kind};
use prost::Message;

#[test]
fn a_uint64_value_has_the_wire_encoding_of_a_uint64() {
    let value = Value {
        kind: Some(Kind::Uint64Value(u64::MAX.cast_signed())),
    };
    // Field 5, varint, then the ten-byte varint of 2^64 - 1: what a uint64
    // field would carry, so int64 loses nothing on the wire.
    let want = [
        0x28, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
    ];
    assert_eq!(value.encode_to_vec(), want);
}

#[test]
fn a_uint64_value_decodes_back_to_the_same_bits() {
    let want = [
        0x28, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0x01,
    ];
    let decoded = Value::decode(want.as_slice());
    assert!(matches!(
        decoded,
        Ok(Value { kind: Some(Kind::Uint64Value(bits)) }) if bits.cast_unsigned() == u64::MAX
    ));
}
