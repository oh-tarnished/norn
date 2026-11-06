//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What the meter reserves against what prost actually holds. The fuzz target
//! `server_request` found a batch create holding three times its reservation.

use std::mem::size_of;

use norn_protocol::entity::{BatchCreateEntitiesRequest, CreateEntityRequest};
use norn_protocol::signal::{BatchCreateSignalsRequest, CreateSignalRequest};
use prost::Message;

use super::reader::EXPANSION;

/// A message of `n` empty elements of field `field`, the most memory per byte.
fn empty_elements(field: u8, n: usize) -> Vec<u8> {
    [field << 3 | 2, 0].repeat(n)
}

#[test]
fn a_batch_create_of_empty_entities_fits_its_reservation() {
    let wire = empty_elements(1, 128 << 10);
    let decoded = BatchCreateEntitiesRequest::decode(wire.as_slice()).expect("it decodes");
    assert_eq!(decoded.requests.len(), 128 << 10);
    let held = decoded.requests.capacity() * size_of::<CreateEntityRequest>();
    assert!(
        held as u64 <= EXPANSION * wire.len() as u64,
        "{held} bytes held"
    );
}

#[test]
fn a_batch_create_of_empty_signals_fits_its_reservation() {
    let wire = empty_elements(2, 128 << 10);
    let decoded = BatchCreateSignalsRequest::decode(wire.as_slice()).expect("it decodes");
    assert_eq!(decoded.requests.len(), 128 << 10);
    let held = decoded.requests.capacity() * size_of::<CreateSignalRequest>();
    assert!(
        held as u64 <= EXPANSION * wire.len() as u64,
        "{held} bytes held"
    );
}
