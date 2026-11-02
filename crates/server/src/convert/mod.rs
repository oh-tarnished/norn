//! Wire ↔ core conversions, per docs/reference/protocol.md "Unsigned values".
//!
//! Protocol enums start at `UNSPECIFIED = 0`, so a quality severity sits one
//! above core's code; a substatus's numbers equal core's, its zero meaning none;
//! a `ValueType` number equals its `ValueKind` tag. Conversions are pure.

mod value;

pub use value::{
    ConvertError, kind_from_wire, kind_to_wire, quality_from_wire, quality_to_wire, sample_to_wire,
    time_from_wire, time_to_wire, value_from_wire, value_to_wire,
};
