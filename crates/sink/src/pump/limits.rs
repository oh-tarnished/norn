//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How large one unary message may be. A daemon that publishes the limit is
//! taken at its word; one that does not, as every daemon up to 1.0.0-rc.1, is
//! taken to hold the limit it is known to have ([`Options::unary_limit`], 256
//! KiB by default). A guess too high is still safe, only slower: a batch the
//! daemon refuses `RESOURCE_EXHAUSTED` is sent again in halves (`provision`).

use crate::options::Options;
use norn_protocol::admin::Limits;

/// The daemon's published limit on one unary message, if it publishes one.
///
/// **The one place to read it.** `NegotiateProtocol`'s `Limits` carries no such
/// field on this base; when the field that says so lands, read it here, a
/// value of zero or less meaning unpublished, and the rest of the sink follows.
pub(super) fn published_message_limit(_limits: &Limits) -> Option<usize> {
    None
}

/// The limit to cut to: the published one, else the known one.
pub(super) fn message_limit(published: Option<usize>, options: &Options) -> usize {
    published.unwrap_or(options.unary_limit)
}

/// The most encoded resources in one batch create under `limit`: three
/// quarters of it, leaving room for the framing an encoded size does not count.
pub(super) fn batch_bytes(limit: usize) -> usize {
    limit / 4 * 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_daemon_publishing_nothing_gets_the_known_limit_cut_to_three_quarters() {
        let options = Options::default();
        let published = published_message_limit(&Limits::default());
        assert_eq!(published, None);
        let limit = message_limit(published, &options);
        assert_eq!(limit, 256 << 10);
        assert_eq!(batch_bytes(limit), 192 << 10);
    }

    #[test]
    fn a_published_limit_wins_over_the_known_one() {
        let options = Options::default().unary_limit(1 << 20);
        assert_eq!(message_limit(Some(64 << 10), &options), 64 << 10);
        assert_eq!(message_limit(None, &options), 1 << 20);
    }
}
