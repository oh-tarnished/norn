//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A ring and a producer in one process, against a model of a bounded queue.
// Rings are never offered on Windows (docs/reference/shared-memory.md).
#![cfg(unix)]
#![allow(missing_docs, clippy::unwrap_used, clippy::expect_used, clippy::panic)]
#![allow(clippy::indexing_slicing)]

use std::collections::VecDeque;
use std::io::ErrorKind;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_ipc::{Attach, DataPlaneKind};
use norn_shared_memory::{
    Producer, Ring, Settings, SharedMemory, SharedMemoryTicket, chunks_per_ring, ring_bytes,
};
use proptest::prelude::*;

struct Dir(std::path::PathBuf);

impl Drop for Dir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ticket(slots: usize, frame_bytes: usize) -> (SharedMemoryTicket, Dir) {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let n = NEXT.fetch_add(1, Ordering::Relaxed);
    let pid = std::process::id();
    let root = std::env::temp_dir().join(format!("nsm-{pid}-{n}/"));
    std::fs::create_dir_all(&root).unwrap();
    let ticket = SharedMemoryTicket {
        service: format!("t/{pid}/{n}"),
        root_directory: root.clone(),
        name_prefix: format!("nsm{pid}_"),
        slot_count: slots,
        max_frame_bytes: frame_bytes,
    };
    (ticket, Dir(root))
}

fn take(ring: &Ring) -> Option<Vec<u8>> {
    ring.next(<[u8]>::to_vec).unwrap()
}

#[tokio::test]
async fn a_frame_round_trips_through_the_data_plane_seam() {
    let (t, _dir) = ticket(2, 64);
    let ring = Ring::create(&t).unwrap();
    let plane = SharedMemory.attach(&t).unwrap();
    assert_eq!(plane.kind(), DataPlaneKind::SharedMemory);
    assert_eq!(plane.max_frame(), 64);
    plane.send(b"a batch").await.unwrap();
    assert_eq!(take(&ring).unwrap(), b"a batch");
    assert_eq!(take(&ring), None);
}

#[test]
fn a_full_ring_refuses_and_loses_nothing() {
    let (t, _dir) = ticket(2, 16);
    let ring = Ring::create(&t).unwrap();
    let producer = Producer::attach(&t).unwrap();
    producer.try_send(b"a").unwrap();
    producer.try_send(b"b").unwrap();
    let full = producer.try_send(b"c").unwrap_err();
    assert_eq!(full.kind(), ErrorKind::WouldBlock);
    assert_eq!(take(&ring).unwrap(), b"a");
    producer.try_send(b"c").unwrap();
    assert_eq!(take(&ring).unwrap(), b"b");
    assert_eq!(take(&ring).unwrap(), b"c");
    assert_eq!(take(&ring), None);
}

#[test]
fn a_frame_over_the_limit_is_refused_by_the_producer() {
    let (t, _dir) = ticket(2, 4);
    let _ring = Ring::create(&t).unwrap();
    let producer = Producer::attach(&t).unwrap();
    let e = producer.try_send(&[0; 5]).unwrap_err();
    assert_eq!(e.kind(), ErrorKind::InvalidInput);
}

#[test]
fn sending_after_the_daemon_left_is_an_error_not_a_silent_success() {
    let (t, _dir) = ticket(2, 16);
    let ring = Ring::create(&t).unwrap();
    let producer = Producer::attach(&t).unwrap();
    drop(ring);
    let e = producer.try_send(b"x").unwrap_err();
    assert_eq!(e.kind(), ErrorKind::BrokenPipe);
}

#[test]
fn a_ring_nobody_created_cannot_be_attached() {
    let (t, _dir) = ticket(2, 16);
    assert!(Producer::attach(&t).is_err());
}

#[test]
fn a_second_producer_is_refused() {
    let (t, _dir) = ticket(2, 16);
    let _ring = Ring::create(&t).unwrap();
    let _first = Producer::attach(&t).unwrap();
    assert!(Producer::attach(&t).is_err());
}

#[test]
fn a_ring_is_never_bigger_than_what_is_charged_for_it() {
    assert_eq!(chunks_per_ring(4), 6);
    let settings = Settings::default();
    assert!(!settings.enabled, "off by default");
    assert_eq!(ring_bytes(&settings), 6 * 1024 * 1024);
}

#[derive(Debug, Clone)]
enum Op {
    Send(Vec<u8>),
    Receive,
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        proptest::collection::vec(any::<u8>(), 0..32).prop_map(Op::Send),
        Just(Op::Receive),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn a_ring_behaves_like_a_bounded_queue(
        ops in proptest::collection::vec(op(), 1..120),
        slots in 1usize..5,
    ) {
        let (t, _dir) = ticket(slots, 32);
        let ring = Ring::create(&t).unwrap();
        let producer = Producer::attach(&t).unwrap();
        let mut model = VecDeque::new();
        for op in ops {
            match op {
                Op::Send(frame) => match producer.try_send(&frame) {
                    Ok(()) => {
                        prop_assert!(model.len() < slots);
                        model.push_back(frame);
                    }
                    Err(e) => {
                        prop_assert_eq!(e.kind(), ErrorKind::WouldBlock);
                        prop_assert_eq!(model.len(), slots);
                    }
                },
                Op::Receive => prop_assert_eq!(take(&ring), model.pop_front()),
            }
        }
    }
}
