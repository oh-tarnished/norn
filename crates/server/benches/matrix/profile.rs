//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The matrix itself: which rows each profile runs, at what scale, how often.
//!
//! `--quick` keeps SPEC's signal counts and rates but runs each row for seconds
//! and builds a narrow week of history, so CI finishes in minutes. `--full` is
//! the owner's run, for hours. With no `--bench` argument (`cargo test
//! --benches`) a smoke profile proves every path in well under a minute.

use norn_protocol::telemetry::AcknowledgementLevel::{
    self, Buffered, Durable, Persisted, Received,
};

/// Which data plane carries an ingest row's batches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Plane {
    Socket,
    SharedMemory,
}

/// One open-loop ingest run on a fresh daemon.
#[derive(Debug, Clone)]
pub struct IngestRow {
    /// `ingest` or `planes`: the first part of every metric key it makes.
    pub group: &'static str,
    pub case: String,
    pub signals: usize,
    /// Offered samples a second, over all signals.
    pub rate: u64,
    pub batch: u64,
    pub secs: u64,
    pub level: AcknowledgementLevel,
    pub plane: Plane,
}

/// The history scenario: load, compact, query, StateAt, replay, kill -9.
#[derive(Debug, Clone)]
pub struct History {
    pub signals: usize,
    /// Seconds of 1 Hz history per signal.
    pub seconds: i64,
    /// Query windows; one longer than the history is skipped and says so.
    pub windows: Vec<(&'static str, i64)>,
    pub rounds: usize,
    /// Seconds of every signal replayed at maximum speed.
    pub replay_seconds: i64,
    /// Seconds of new samples loaded before the kill -9.
    pub crash_seconds: i64,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub name: &'static str,
    pub repeats: usize,
    pub rows: Vec<IngestRow>,
    pub history: History,
}

pub const HOUR: i64 = 3_600;
pub const DAY: i64 = 86_400;
pub const WEEK: i64 = 7 * DAY;

fn level_name(level: AcknowledgementLevel) -> &'static str {
    match level {
        Received => "received",
        Buffered => "buffered",
        Persisted => "persisted",
        _ => "durable",
    }
}

fn row(case: &str, signals: usize, rate: u64, secs: u64, level: AcknowledgementLevel) -> IngestRow {
    IngestRow {
        group: "ingest",
        case: format!("{case}.{}", level_name(level)),
        signals,
        rate,
        batch: (rate / 10).clamp(100, 10_000),
        secs,
        level,
        plane: Plane::Socket,
    }
}

fn plane(signals: usize, rate: u64, batch: u64, secs: u64, plane: Plane) -> IngestRow {
    let name = if plane == Plane::Socket {
        "socket"
    } else {
        "shared_memory"
    };
    IngestRow {
        group: "planes",
        case: format!("{rate}_per_s.{name}"),
        signals,
        rate,
        batch,
        secs,
        level: Buffered,
        plane,
    }
}

/// SPEC §15, §17 and §35's rows, each run for the given seconds.
fn matrix(sustained: u64, burst: u64, scale: u64, planes: u64) -> Vec<IngestRow> {
    let mut rows = Vec::new();
    for level in [Received, Buffered, Persisted, Durable] {
        rows.push(row("sustained_16000x1hz", 16_000, 16_000, sustained, level));
    }
    for level in [Buffered, Durable] {
        rows.push(row("burst_10x", 16_000, 160_000, burst, level));
        rows.push(row("burst_100x", 16_000, 1_600_000, burst, level));
    }
    for signals in [1_000, 10_000, 100_000] {
        let case = format!("signals_{signals}x1hz");
        rows.push(row(&case, signals, signals as u64, scale, Buffered));
    }
    for (rate, batch) in [(1_000, 100), (10_000, 1_000), (100_000, 2_000)] {
        rows.push(plane(16_000, rate, batch, planes, Plane::Socket));
        rows.push(plane(16_000, rate, batch, planes, Plane::SharedMemory));
    }
    rows
}

fn windows() -> Vec<(&'static str, i64)> {
    vec![("1h", HOUR), ("1d", DAY), ("1w", WEEK)]
}

pub fn quick() -> Profile {
    Profile {
        name: "quick",
        repeats: 3,
        rows: matrix(2, 2, 2, 2),
        history: History {
            signals: 4,
            seconds: WEEK,
            windows: windows(),
            rounds: 5,
            replay_seconds: WEEK,
            crash_seconds: 60,
        },
    }
}

pub fn full() -> Profile {
    Profile {
        name: "full",
        repeats: 3,
        rows: matrix(300, 30, 120, 30),
        history: History {
            signals: 1_000,
            seconds: WEEK,
            windows: windows(),
            rounds: 50,
            replay_seconds: DAY,
            crash_seconds: 600,
        },
    }
}

/// Every kind of row once, tiny, on a debug build.
pub fn smoke() -> Profile {
    Profile {
        name: "smoke",
        repeats: 1,
        rows: vec![
            row("sustained_16000x1hz", 20, 200, 1, Received),
            row("sustained_16000x1hz", 20, 200, 1, Durable),
            row("burst_10x", 20, 2_000, 1, Buffered),
            plane(20, 1_000, 100, 1, Plane::Socket),
            plane(20, 1_000, 100, 1, Plane::SharedMemory),
        ],
        history: History {
            signals: 2,
            seconds: 2 * HOUR,
            windows: windows(),
            rounds: 2,
            replay_seconds: HOUR,
            crash_seconds: 30,
        },
    }
}
