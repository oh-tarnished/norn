//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The text a request carries past protobuf: filters and page tokens. Any string
//! parses or is refused by name, and a position encodes back to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::MAX_INPUT;
use norn_query::{Filter, Position};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let text = String::from_utf8_lossy(data);
    let _ = Filter::parse(&text);
    if let Some(position) = Position::parse(&text) {
        let encoded = position.encode();
        assert_eq!(
            Position::parse(&encoded),
            Some(position),
            "from {encoded:?}"
        );
    }
});
