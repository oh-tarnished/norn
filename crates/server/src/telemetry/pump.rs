//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The blocking task that empties one ring, treating everything in it as hostile.
//!
//! Blocking, because iceoryx2 has no async API; a task of its own, on the calls'
//! tracker, because iceoryx2 answers some corruption by panicking: the panic
//! ends this task and so its call, not the daemon. It cannot help with a bad
//! pointer: see docs/reference/shared-memory.md.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use norn_shared_memory::Ring;
use tokio::runtime::Handle;
use tokio::sync::mpsc;
use tonic::Status;

pub(crate) use super::arrive::Arriving;
use super::arrive::{decode, invalid, take};
use super::plane::Plane;

/// Empty polls answered at once, then with a short sleep, then a long one: a busy
/// ring costs a fraction of one core, an idle one next to nothing.
const SPINS: u32 = 64;
const NAPS: u32 = 4_096;
const NAP: Duration = Duration::from_micros(20);
const SLEEP: Duration = Duration::from_millis(1);

/// What the reader tells the call.
#[derive(Debug)]
pub(crate) enum Item {
    /// The next batch.
    Batch(Box<Arriving>),
    /// The ring was empty after [`Control::flush`].
    Flushed,
    /// The ring cannot go on; the call ends with this.
    Failed(Status),
    /// The reader has stopped and let go of the ring.
    Done,
}

/// What the call tells the reader.
#[derive(Debug, Default)]
pub(crate) struct Control {
    stop: AtomicBool,
    flush: AtomicBool,
}

impl Control {
    pub(super) fn new() -> Arc<Self> {
        Arc::default()
    }

    /// Asks for what is in the ring, then an end.
    pub(super) fn stop(&self) {
        self.stop.store(true, Ordering::Release);
    }

    /// Asks for [`Item::Flushed`] once what is in the ring has been delivered.
    pub(super) fn flush(&self) {
        self.flush.store(true, Ordering::Release);
    }
}

/// A call's end of a ring.
#[derive(Debug)]
pub(crate) struct Attached {
    pub(super) frames: mpsc::Receiver<Item>,
    pub(super) control: Arc<Control>,
}

fn wait(idle: &mut u32) {
    *idle = idle.saturating_add(1);
    if *idle < SPINS {
        std::hint::spin_loop();
    } else if *idle < NAPS {
        std::thread::sleep(NAP);
    } else {
        std::thread::sleep(SLEEP);
    }
}

/// Moves batches to `items` until stopped or the ring fails. A slot is released
/// as its frame is copied, and the channel is small, so a daemon that is behind
/// fills the ring and the producer waits: backpressure, not loss. After `stop` it
/// reads at most what a ring can hold, so a producer writing on cannot keep it
/// alive.
pub(super) fn run(
    ring: &Ring,
    plane: &Plane,
    handle: &Handle,
    items: &mpsc::Sender<Item>,
    control: &Control,
) {
    let mut idle = 0;
    let mut left = plane.slots().saturating_add(1);
    loop {
        let stopping = control.stop.load(Ordering::Acquire);
        if stopping && left == 0 {
            return;
        }
        let item = match ring.next(|frame| take(frame, &plane.budget, plane.limits, handle)) {
            Ok(Some(Ok((payload, paid)))) => decode(payload, paid, plane.limits),
            Ok(Some(Err(status))) => Err(status),
            Err(e) => Err(invalid(e.to_string())),
            Ok(None) if stopping => return,
            Ok(None) => {
                if control.flush.swap(false, Ordering::AcqRel)
                    && items.blocking_send(Item::Flushed).is_err()
                {
                    return;
                }
                wait(&mut idle);
                continue;
            }
        };
        idle = 0;
        if stopping {
            left = left.saturating_sub(1);
        }
        let sent = match item {
            Ok(arriving) => items.blocking_send(Item::Batch(Box::new(arriving))),
            Err(status) => {
                let _ = items.blocking_send(Item::Failed(status));
                return;
            }
        };
        if sent.is_err() {
            return;
        }
    }
}
