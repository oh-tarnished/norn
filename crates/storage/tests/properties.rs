//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! decode(encode(x)) == x over random sample streams, through real files.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{Scratch, bytes_of, read_all, sealed};
use norn_core::{Quality, Sample, Sequence, Severity, SignalId, Substatus, Timestamp, Value};
use norn_storage::{Chunk, RecoveryMode, StorageOptions, compact, recover_stream};
use proptest::prelude::*;

/// Values of one kind, per `kind`.
fn value(kind: u8) -> BoxedStrategy<Value> {
    match kind {
        0 => any::<bool>().prop_map(Value::Bool).boxed(),
        1 => any::<i32>().prop_map(Value::I32).boxed(),
        2 => any::<i64>().prop_map(Value::I64).boxed(),
        3 => any::<u32>().prop_map(Value::U32).boxed(),
        4 => any::<u64>().prop_map(Value::U64).boxed(),
        5 => any::<u32>()
            .prop_map(|b| Value::F32(f32::from_bits(b)))
            .boxed(),
        6 => any::<u64>()
            .prop_map(|b| Value::F64(f64::from_bits(b)))
            .boxed(),
        7 => ".{0,8}".prop_map(Value::String).boxed(),
        8 => proptest::collection::vec(any::<u8>(), 0..8)
            .prop_map(Value::Bytes)
            .boxed(),
        _ => any::<u32>().prop_map(Value::Enum).boxed(),
    }
}

/// One signal's stream: arbitrary timestamps (out of order included).
fn stream() -> impl Strategy<Value = Vec<Vec<Sample>>> {
    let signal = (1u64..6, 0u8..10).prop_flat_map(|(id, kind)| {
        let row = (any::<i64>(), value(kind), any::<u64>(), any::<bool>());
        proptest::collection::vec(row, 1..200).prop_map(move |rows| {
            rows.into_iter()
                .map(|(t, v, q, bad)| {
                    let quality = if bad {
                        Quality::new(Severity::Bad, Substatus::DeviceFailure)
                    } else {
                        Quality::GOOD
                    };
                    let (ts, seq) = (Timestamp::from_nanos(t), Sequence::new(q));
                    Sample::new(ts, SignalId::new(id), v, quality, seq)
                })
                .collect::<Vec<_>>()
        })
    });
    proptest::collection::vec(signal, 1..6)
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn random_streams_round_trip_through_a_sealed_segment(groups in stream()) {
        let dir = Scratch::new();
        let chunks: Vec<Chunk> = groups.iter().map(|g| Chunk::from_samples(g).unwrap()).collect();
        let seg = sealed(dir.path(), 0, &chunks);
        let want: Vec<Sample> = groups.iter().flatten().cloned().collect();
        prop_assert_eq!(bytes_of(&read_all(&seg.path)), bytes_of(&want));
        let report =
            recover_stream(dir.path(), &StorageOptions::default(), RecoveryMode::ReportOnly)
                .unwrap();
        prop_assert!(report.is_clean());
    }

    #[test]
    fn compaction_keeps_every_random_sample(groups in stream()) {
        let dir = Scratch::new();
        let chunks: Vec<Chunk> = groups.iter().map(|g| Chunk::from_samples(g).unwrap()).collect();
        let seg = sealed(dir.path(), 0, &chunks);
        let opts = StorageOptions::default();
        let done = compact(dir.path(), std::slice::from_ref(&seg.path), &opts).unwrap();
        let mut got = bytes_of(&read_all(&done.output.path));
        let mut want = bytes_of(&groups.iter().flatten().cloned().collect::<Vec<_>>());
        got.sort();
        want.sort();
        prop_assert_eq!(got, want);
    }
}
