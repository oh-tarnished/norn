//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What a stream's admission and its two threads share.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize};

use norn_core::{Sample, StreamId};
use tokio::sync::oneshot;

use super::buffer::Buffers;
use super::decision::{AckFailure, AckLevel};
use super::stats::StreamStats;
use super::tail::Tail;
use crate::budget::Reservation;

/// One admitted batch on its way to the commit thread.
#[derive(Debug)]
pub(crate) struct Work {
    pub(crate) samples: Vec<Sample>,
    pub(crate) memory: Reservation,
    pub(crate) level: AckLevel,
    pub(crate) ack: Option<oneshot::Sender<Result<AckLevel, AckFailure>>>,
}

/// Counters read lock-free by admission.
#[derive(Debug)]
pub(crate) struct Shared {
    pub(crate) stream: StreamId,
    /// Batches admitted and not yet taken by the commit thread.
    pub(crate) queued: AtomicUsize,
    /// Reserved bytes admitted and not yet stored.
    pub(crate) held: AtomicU64,
    /// Nanoseconds on the clock until which batches are refused as `Throttled`.
    pub(crate) throttled_until: AtomicI64,
    /// Storage failed; every further batch is refused until restart.
    pub(crate) failed: AtomicBool,
    /// The index could not take this stream's chunks: backpressure, until a
    /// refresh succeeds. The chunks themselves are safe on disk.
    pub(crate) index_behind: AtomicBool,
    pub(crate) stats: Mutex<StreamStats>,
    /// What queries read until the index has it.
    pub(crate) tail: Tail,
}

impl Shared {
    pub(crate) fn new(stream: StreamId, stats: StreamStats, buffers: Buffers) -> Self {
        Self {
            stream,
            queued: AtomicUsize::new(0),
            held: AtomicU64::new(0),
            throttled_until: AtomicI64::new(i64::MIN),
            failed: AtomicBool::new(false),
            index_behind: AtomicBool::new(false),
            stats: Mutex::new(stats),
            tail: Tail::new(buffers),
        }
    }
}
