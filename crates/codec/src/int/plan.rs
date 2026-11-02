//! Choosing a miniblock's reference and width, with patched exceptions
//! (PFOR; Zukowski et al., ICDE 2006).
//!
//! Plain frame-of-reference packs every value at the width of the
//! miniblock's full range, so one outlier (a jittered timestamp, a gap, a
//! counter reset) widens all 128 values. A patched miniblock instead packs
//! the values inside `[base, base + 2^width)` and stores the few outside it
//! as exceptions: a one-byte position and a zigzag varint each. The plan
//! tries up to [`MAX_EXCEPTIONS`] values trimmed from either tail of the
//! sorted miniblock and keeps the cheapest, so an outlier-free miniblock
//! still gets plain FOR.

use crate::bits::{mask, packed_len, width_of};
use crate::wire::zigzag;

/// The most values trimmed into exceptions when planning a miniblock.
const MAX_EXCEPTIONS: usize = 16;

/// How one miniblock is packed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Plan {
    /// The reference subtracted from every in-range value.
    pub base: i64,
    /// Bits per packed value, 0 to 64.
    pub width: u32,
}

impl Plan {
    /// Whether `v` lies outside the packed range and must be patched.
    pub(crate) fn is_exception(self, v: i64) -> bool {
        v.wrapping_sub(self.base) as u64 > mask(self.width)
    }

    /// Exact encoded bytes of `chunk` under this plan.
    pub(crate) fn size(self, chunk: &[i64]) -> usize {
        let exc: Vec<i64> = chunk
            .iter()
            .copied()
            .filter(|v| self.is_exception(*v))
            .collect();
        let packed = packed_len(chunk.len(), self.width).unwrap_or(usize::MAX);
        let patch = match exc.len() {
            0 => 0,
            n => 1 + n + exc.iter().map(|v| exc_len(*v, self.base)).sum::<usize>(),
        };
        varint_len(zigzag(self.base)) + 1 + packed + patch
    }
}

/// Bytes a varint of `v` occupies.
pub(crate) fn varint_len(v: u64) -> usize {
    (width_of(v).max(1) as usize).div_ceil(7)
}

/// Varint bytes of an exception value stored relative to `base`.
fn exc_len(v: i64, base: i64) -> usize {
    varint_len(zigzag(v.wrapping_sub(base)))
}

/// The cheapest plan for a non-empty miniblock.
///
/// Fast paths: a miniblock whose full range packs in at most one bit per
/// value cannot be beaten by patching and returns at once; inside the
/// search, a candidate whose exception bytes alone already reach the best
/// cost so far stops that branch.
pub(crate) fn plan(chunk: &[i64]) -> Plan {
    let (lo_v, hi_v) = chunk
        .iter()
        .fold((i64::MAX, i64::MIN), |(a, b), v| (a.min(*v), b.max(*v)));
    let full = Plan {
        base: lo_v,
        width: width_of(hi_v.wrapping_sub(lo_v) as u64),
    };
    if full.width <= 1 {
        return full;
    }
    let mut s = chunk.to_vec();
    s.sort_unstable();
    let n = s.len();
    let at = |i: usize| s.get(i).copied().unwrap_or(0);
    let mut best = (usize::MAX, full);
    let k = MAX_EXCEPTIONS.min(n.saturating_sub(1));
    for lo in 0..=k {
        let base = at(lo);
        let low_cost: usize = s.iter().take(lo).map(|v| 1 + exc_len(*v, base)).sum();
        let mut high_cost = 0usize;
        for hi in 0..=(k - lo) {
            if hi > 0 {
                high_cost += 1 + exc_len(at(n - hi), base);
            }
            let patch = usize::from(lo + hi > 0) + low_cost + high_cost;
            if patch >= best.0 {
                break;
            }
            let width = width_of(at(n - 1 - hi).wrapping_sub(base) as u64);
            let packed = packed_len(n, width).unwrap_or(usize::MAX);
            let cost = varint_len(zigzag(base)) + 1 + packed + patch;
            if cost < best.0 {
                best = (cost, Plan { base, width });
            }
        }
    }
    best.1
}
