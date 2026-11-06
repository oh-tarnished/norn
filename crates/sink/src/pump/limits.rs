//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! How large one unary message may be. A daemon that publishes the limit is
//! taken at its word; one that does not, as every daemon before protocol 1.8, is
//! taken to hold the limit it is known to have ([`Options::unary_limit`], 256
//! KiB by default). A guess too high is still safe, only slower: a batch the
//! daemon refuses `RESOURCE_EXHAUSTED` is sent again in halves (`provision`).

use crate::options::Options;
use norn_protocol::admin::Limits;

/// The daemon's published limit on one unary message, if it publishes one.
///
/// **The one place to read it.** `Limits.max_message_bytes`, added in protocol
/// 1.8; a daemon before 1.8 leaves it zero, and zero or less means unpublished.
pub(super) fn published_message_limit(limits: &Limits) -> Option<usize> {
    usize::try_from(limits.max_message_bytes)
        .ok()
        .filter(|&bytes| bytes > 0)
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
    fn a_daemon_publishing_its_limit_is_taken_at_its_word() {
        let limits = Limits {
            max_message_bytes: 64 << 10,
            ..Limits::default()
        };
        assert_eq!(published_message_limit(&limits), Some(64 << 10));
        for unpublished in [0, -1] {
            let limits = Limits {
                max_message_bytes: unpublished,
                ..Limits::default()
            };
            assert_eq!(published_message_limit(&limits), None);
        }
    }

    #[test]
    fn a_published_limit_wins_over_the_known_one() {
        let options = Options::default().unary_limit(1 << 20);
        assert_eq!(message_limit(Some(64 << 10), &options), 64 << 10);
        assert_eq!(message_limit(None, &options), 1 << 20);
    }
}
