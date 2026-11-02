//! A held reservation, released on drop.

use super::Budget;

/// Bytes held against a [`Budget`]; dropping it returns them.
///
/// Concurrency: `Send + Sync`; it may move to whichever task ends up
/// holding the memory it accounts for.
#[derive(Debug)]
#[must_use = "dropping a reservation releases its bytes at once"]
pub struct Reservation {
    pub(super) budget: Budget,
    pub(super) bytes: u64,
}

impl Reservation {
    /// The bytes held.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Returns all but `keep` bytes to the budget now, as when an estimate
    /// turns out larger than the real size. A `keep` above the held amount
    /// changes nothing: a reservation never grows without asking.
    pub fn shrink_to(&mut self, keep: u64) {
        if keep < self.bytes {
            self.budget.release(self.bytes - keep);
            self.bytes = keep;
        }
    }

    /// Takes over `other`'s bytes, so a long-lived holder of many small
    /// reservations keeps one value. Both must be of the same budget, which is
    /// why this is crate-private.
    pub(crate) fn merge(&mut self, mut other: Reservation) {
        self.bytes = self.bytes.saturating_add(other.bytes);
        other.bytes = 0;
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        self.budget.release(self.bytes);
    }
}
