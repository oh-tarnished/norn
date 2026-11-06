//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The index's own files, as its encoders write them.

use norn_core::{SignalId, StreamId, Timestamp, ValueKind};
use norn_index::SegmentRecord;
use norn_index::SignalSpan;
use norn_index::fuzzing::{SUMMARY_RECORD_LEN, encode_detail_file, encode_signals, encode_summary};
use norn_storage::SegmentName;
use norn_storage::format::frame::ChunkMeta;

use crate::Corpus;

const STREAM: StreamId = StreamId::new(7);
/// Magic, version, reserved, count, stream: where a summary's records start.
const SUMMARY_HEAD: usize = 24;

pub fn write(c: &Corpus) {
    let metas: Vec<ChunkMeta> = (0..300u32)
        .map(|i| ChunkMeta {
            offset: 64 + u64::from(i) * 256,
            body_len: 200,
            signal: SignalId::new(u64::from(i / 8)),
            kind: ValueKind::F64,
            min_ts: Timestamp::from_nanos(i64::from(i) * 100),
            max_ts: Timestamp::from_nanos(i64::from(i) * 100 + 99),
            count: 16,
        })
        .collect();
    let record = SegmentRecord {
        first_seq: 1,
        last_seq: 1,
        generation: 0,
        chunks: metas.len() as u32,
        samples: 16 * metas.len() as u64,
        file_len: 76_800,
        min_ts: Timestamp::from_nanos(0),
        max_ts: Timestamp::from_nanos(29_999),
        min_signal: SignalId::new(0),
        max_signal: SignalId::new(37),
    };
    let summary = encode_summary(STREAM, &[record, record]);
    c.selected("index_summary", "two", 1, &summary);

    let span = SignalSpan {
        first: Timestamp::from_nanos(0),
        last: Timestamp::from_nanos(100),
        last_segment: SegmentName {
            seq: 1,
            generation: 0,
        },
    };
    let entries = (0..8).map(|i| (SignalId::new(i), span)).collect();
    c.selected(
        "index_signals",
        "eight",
        1,
        &encode_signals(STREAM, 0x1234, entries),
    );

    let mut sorted = metas.clone();
    let sidecar = encode_detail_file(STREAM, &record, &mut sorted);
    let raw = summary
        .get(SUMMARY_HEAD..SUMMARY_HEAD + SUMMARY_RECORD_LEN)
        .unwrap_or_default();
    let mut seed = raw.to_vec();
    seed.extend_from_slice(&sidecar);
    c.selected("index_detail", "sidecar", 1, &seed);

    let mut tail = Vec::new();
    for i in 0..600u32 {
        tail.extend_from_slice(&[6, 8, 0, (i / 8) as u8, (i % 200) as u8, 40, 0, 200]);
    }
    c.selected("index_tail", "chunks", 3, &tail);
}
