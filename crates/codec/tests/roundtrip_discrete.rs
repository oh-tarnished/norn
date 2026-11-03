//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Property tests: decode(encode(x)) == x for boolean, enum and byte-string
//! codecs, and for the compression envelope at every level.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

mod common;

use common::*;
use norn_codec::compress::{self, Compression, Stored, ZstdLevel};
use norn_codec::{ColumnType, boolean, bytes, enumeration};
use proptest::prelude::*;

fn bools() -> impl Strategy<Value = Vec<bool>> {
    prop_oneof![
        prop::collection::vec(any::<bool>(), 0..700),
        prop::collection::vec((any::<bool>(), 1usize..300), 0..8).prop_map(|r| r
            .into_iter()
            .flat_map(|(b, n)| std::iter::repeat_n(b, n))
            .collect()),
    ]
}

fn enums() -> impl Strategy<Value = Vec<u32>> {
    let code = prop_oneof![0u32..6, any::<u32>(), Just(u32::MAX)];
    prop_oneof![
        prop::collection::vec(code.clone(), 0..700),
        prop::collection::vec((code, 1usize..200), 0..10).prop_map(|r| r
            .into_iter()
            .flat_map(|(c, n)| std::iter::repeat_n(c, n))
            .collect()),
    ]
}

fn byte_strings() -> impl Strategy<Value = Vec<Vec<u8>>> {
    let word = prop_oneof![
        prop::collection::vec(any::<u8>(), 0..40),
        Just(b"occupied".to_vec()),
        Just(b"unoccupied".to_vec()),
        Just(Vec::new()),
    ];
    prop::collection::vec(word, 0..300)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(300))]

    #[test]
    fn booleans_round_trip_under_every_codec(values in bools()) {
        let mut block = Vec::new();
        boolean::encode(&values, &mut block).unwrap();
        let mut back = Vec::new();
        boolean::decode(&block, &mut back).unwrap();
        prop_assert_eq!(&back, &values);
        for codec in codecs(ColumnType::Boolean) {
            let (mut block, mut back) = (Vec::new(), Vec::new());
            boolean::encode_with(codec, &values, &mut block).unwrap();
            boolean::decode(&block, &mut back).unwrap();
            prop_assert_eq!(&back, &values);
        }
    }

    #[test]
    fn enums_round_trip_under_every_codec(values in enums()) {
        let mut block = Vec::new();
        enumeration::encode(&values, &mut block).unwrap();
        let mut back = Vec::new();
        enumeration::decode(&block, &mut back).unwrap();
        prop_assert_eq!(&back, &values);
        for codec in codecs(ColumnType::Enum) {
            let (mut block, mut back) = (Vec::new(), Vec::new());
            enumeration::encode_with(codec, &values, &mut block).unwrap();
            enumeration::decode(&block, &mut back).unwrap();
            prop_assert_eq!(&back, &values);
        }
    }

    #[test]
    fn byte_strings_round_trip_under_every_codec(values in byte_strings()) {
        let mut block = Vec::new();
        bytes::encode(&values, &mut block).unwrap();
        let mut back = Vec::new();
        bytes::decode(&block, &mut back).unwrap();
        prop_assert_eq!(&back, &values);
        for codec in codecs(ColumnType::Bytes) {
            let (mut block, mut back) = (Vec::new(), Vec::new());
            bytes::encode_with(codec, &values, &mut block).unwrap();
            bytes::decode(&block, &mut back).unwrap();
            prop_assert_eq!(&back, &values);
        }
    }

    #[test]
    fn envelopes_round_trip_at_every_level(block in prop::collection::vec(0u8..4, 0..5000)) {
        let policies = [3, 4, 5, 6].map(|l| Compression::Zstd(ZstdLevel::new(l).unwrap()));
        let all = [Compression::None, Compression::Default, Compression::ZstdSmallest];
        for how in all.into_iter().chain(policies) {
            let (mut sealed, mut opened) = (Vec::new(), Vec::new());
            let stored = compress::seal(&block, how, &mut sealed).unwrap();
            prop_assert_eq!(compress::open(&sealed, &mut opened).unwrap(), stored);
            prop_assert_eq!(&opened, &block);
            if let Stored::Zstd(_) = stored {
                prop_assert!(sealed.len() < block.len() + 2 + 3);
            }
        }
    }
}
