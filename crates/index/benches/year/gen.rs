//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Writes synthetic segment files: real header, directory and trailer, no bodies.

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use norn_core::StreamId;
use norn_storage::format::{DIR_MAGIC, TRAILER_MAGIC};
use norn_storage::{SegmentHeader, SegmentName, crc32c, stream_dir};

pub const STREAM: StreamId = StreamId::new(1);
pub const START_NANOS: i64 = 1_767_225_600_000_000_000; // 2026-01-01T00:00:00Z
pub const HOUR_NANOS: i64 = 3_600_000_000_000;
const BYTES_PER_SAMPLE: f64 = 1.2;
const F64_TAG: u8 = norn_core::ValueKind::F64.tag();

/// How signals, hours and chunks are arranged.
#[derive(Debug, Clone, Copy)]
pub struct Layout {
    pub name: &'static str,
    pub signals: u64,
    pub hours: u64,
    pub segments_per_hour: u64,
    pub chunks_per_segment: u64,
}

impl Layout {
    /// docs/reference/storage.md: 16,000 chunks in one segment an hour.
    pub fn compacted(signals: u64, hours: u64) -> Self {
        Self {
            name: "compacted",
            signals,
            hours,
            segments_per_hour: 1,
            chunks_per_segment: signals,
        }
    }

    /// docs/reference/storage.md: 368,000 chunks an hour, so 23 per signal per hour, in four
    /// segments of 92,000.
    pub fn ingest(signals: u64, hours: u64) -> Self {
        Self {
            name: "ingest",
            signals,
            hours,
            segments_per_hour: 4,
            chunks_per_segment: signals * 23 / 4,
        }
    }

    pub fn segments(&self) -> u64 {
        self.hours * self.segments_per_hour
    }

    pub fn chunks(&self) -> u64 {
        self.segments() * self.chunks_per_segment
    }

    fn segment_bytes(&self, seq: u64, out: &mut Vec<u8>) {
        out.clear();
        let header = SegmentHeader::new(STREAM, seq);
        out.extend_from_slice(&header.encode());
        let dir_at = out.len();
        out.extend_from_slice(&DIR_MAGIC);
        let count = self.chunks_per_segment;
        out.extend_from_slice(&(count as u32).to_le_bytes());
        let span = HOUR_NANOS / self.segments_per_hour as i64;
        let seg_start = START_NANOS + seq as i64 * span;
        // Each signal gets a chunk per slice; the last slice may be partial.
        let slice_nanos = span * self.signals as i64 / count as i64;
        let samples = (slice_nanos / 1_000_000_000) as u32;
        let body = (f64::from(samples) * BYTES_PER_SAMPLE) as u32;
        let mut offset = 48u64;
        for i in 0..count {
            let (signal, slice) = (i % self.signals, (i / self.signals) as i64);
            let min = seg_start + slice * slice_nanos + (signal as i64 % 1000) * 1_000_000;
            let max = min + i64::from(samples - 1) * 1_000_000_000;
            out.extend_from_slice(&offset.to_le_bytes());
            out.extend_from_slice(&body.to_le_bytes());
            // Even ids only: an odd id is inside every segment's range and in
            // none, which is the worst case for a nearest lookup.
            out.extend_from_slice(&(signal * 2).to_le_bytes());
            out.extend_from_slice(&min.to_le_bytes());
            out.extend_from_slice(&max.to_le_bytes());
            out.extend_from_slice(&samples.to_le_bytes());
            out.extend([F64_TAG, 0, 0, 0]);
            offset += 48 + u64::from(body);
        }
        let crc = crc32c(&out[dir_at..]);
        out.extend_from_slice(&crc.to_le_bytes());
        out.extend_from_slice(&(dir_at as u64).to_le_bytes());
        out.extend_from_slice(&TRAILER_MAGIC);
    }
}

/// Writes every segment under `root`; returns the bytes written.
pub fn write(root: &Path, layout: &Layout) -> u64 {
    let dir = stream_dir(root, STREAM);
    fs::create_dir_all(&dir).unwrap();
    let next = AtomicU64::new(0);
    let bytes = AtomicU64::new(0);
    let work = || {
        let mut buf = Vec::new();
        loop {
            let seq = next.fetch_add(1, Ordering::Relaxed);
            if seq >= layout.segments() {
                return;
            }
            layout.segment_bytes(seq, &mut buf);
            let name = SegmentName { seq, generation: 0 }.file_name();
            fs::write(dir.join(name), &buf).unwrap();
            bytes.fetch_add(buf.len() as u64, Ordering::Relaxed);
        }
    };
    std::thread::scope(|s| {
        for _ in 0..8 {
            s.spawn(work);
        }
    });
    bytes.into_inner()
}
