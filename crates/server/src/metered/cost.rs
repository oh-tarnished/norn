//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What decoding a batch costs, shared by every plane that carries one, so a
//! batch over shared memory is paid for exactly as one over the socket.

use std::mem::size_of;

use norn_protocol::common::Sample;
use norn_runtime::Reservation;

use super::MeterLimits;
use super::wire::count_samples;

/// The most decoding an `AppendSamples` message of `length` bytes can cost.
pub(crate) fn worst_batch_cost(length: usize, limits: MeterLimits) -> u64 {
    let wide = length as u64;
    let per_sample = size_of::<Sample>() as u64;
    2 * wide + (wide / 2).min(limits.max_batch_samples as u64) * per_sample
}

/// What counting a batch's samples found.
pub(crate) enum Settled {
    /// It fits; the reservation now holds its real cost.
    Fits,
    /// It has this many samples, too many: decode only what is left without
    /// them, so the handler refuses it by its `batch_id`.
    Oversized(u64),
}

/// Counts `payload`'s samples and cuts `reservation` down to what decoding it
/// will cost. Borrows `payload`; allocates nothing.
pub(crate) fn settle_batch(
    payload: &[u8],
    reservation: &mut Reservation,
    limits: MeterLimits,
) -> Settled {
    let wide = payload.len() as u64;
    let samples = count_samples(payload);
    let cost = 2 * wide + samples.saturating_mul(size_of::<Sample>() as u64);
    if samples > limits.max_batch_samples as u64 || cost > reservation.bytes() {
        reservation.shrink_to(2 * wide);
        return Settled::Oversized(samples);
    }
    reservation.shrink_to(cost.max(1));
    Settled::Fits
}
