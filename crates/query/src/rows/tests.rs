//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Of two rows equal in signal, time and sequence, the first written is kept,
//! whatever order the chunks were read in.
#![allow(clippy::unwrap_used)]

use std::sync::Arc;

use norn_core::{Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value, ValueKind};
use norn_index::ChunkLocator;
use norn_storage::{Chunk, ChunkMeta, SegmentName};

use super::Rows;
use crate::filter::Filter;
use crate::memory::{Charge, QueryMemory, Refused};

#[derive(Debug)]
struct Free;

impl QueryMemory for Free {
    fn acquire(&self, _: u64) -> Result<(), Refused> {
        Ok(())
    }
    fn try_acquire(&self, _: u64) -> Result<(), Refused> {
        Ok(())
    }
    fn release(&self, _: u64) {}
}

fn sample(value: f64) -> Sample {
    let at = Timestamp::from_nanos(5);
    let signal = SignalId::new(1);
    Sample::new(
        at,
        signal,
        Value::F64(value),
        Quality::GOOD,
        Sequence::new(7),
    )
}

/// `chunk`, as if read from the segment of sequence `seq` at `offset`.
fn at(chunk: &Chunk, seq: u64, offset: u64) -> ChunkLocator {
    let (min_ts, max_ts) = (Timestamp::from_nanos(5), Timestamp::from_nanos(5));
    let count = u32::try_from(chunk.len()).unwrap();
    let chunk = ChunkMeta {
        offset,
        body_len: 0,
        signal: SignalId::new(1),
        kind: ValueKind::F64,
        min_ts,
        max_ts,
        count,
    };
    let segment = SegmentName { seq, generation: 0 };
    ChunkLocator {
        stream: StreamId::new(1),
        segment,
        sealed: true,
        chunk,
    }
}

fn kept(feed: impl FnOnce(&mut Rows<'_>)) -> Vec<Value> {
    let filter = Filter::default();
    let span = (Timestamp::from_nanos(0), Timestamp::from_nanos(10));
    let mut rows = Rows::new(Charge::new(Arc::new(Free)), &filter, u64::MAX, span);
    feed(&mut rows);
    rows.finish()
        .0
        .into_iter()
        .map(|r| r.sample.value)
        .collect()
}

#[test]
fn the_older_segments_copy_wins_though_the_newer_is_read_first() {
    let (old, new) = (
        Chunk::from_samples(&[sample(1.0)]).unwrap(),
        Chunk::from_samples(&[sample(2.0)]).unwrap(),
    );
    let got = kept(|rows| {
        rows.take(0, &new, &at(&new, 9, 40)).unwrap();
        rows.take(0, &old, &at(&old, 3, 40)).unwrap();
    });
    assert_eq!(got, [Value::F64(1.0)]);
}

#[test]
fn inside_a_compacted_chunk_the_earlier_place_wins() {
    let merged = Chunk::from_samples(&[sample(1.0), sample(2.0)]).unwrap();
    let got = kept(|rows| rows.take(0, &merged, &at(&merged, 3, 40)).unwrap());
    assert_eq!(got, [Value::F64(1.0)]);
}

#[test]
fn a_stored_copy_wins_over_a_buffered_one() {
    let stored = Chunk::from_samples(&[sample(1.0)]).unwrap();
    let got = kept(|rows| {
        rows.offer(0, sample(3.0)).unwrap();
        rows.take(0, &stored, &at(&stored, 3, 40)).unwrap();
    });
    assert_eq!(got, [Value::F64(1.0)]);
}
