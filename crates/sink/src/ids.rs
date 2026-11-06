//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Resource IDs, as Norn takes them, and names built from them.
//!
//! An ID is AIP-122's: 1 to 63 of `[a-z0-9-]`, starting with a lower-case
//! letter, which the daemon enforces (`norn-metadata`'s `check_resource_id`).
//! The sink checks the same rule at [`Sink::declare`](crate::Sink::declare), so
//! a mistyped ID is a typed error at the call that made it, not a refusal the
//! daemon gives seconds later to a background loop.

/// The longest ID, in bytes.
pub const MAX_ID: usize = 63;

/// Whether `id` is an ID Norn accepts.
#[must_use]
pub fn is_valid(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= MAX_ID
        && id.starts_with(|c: char| c.is_ascii_lowercase())
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
}

/// `text` in an ID's alphabet: ASCII letters and digits lower-cased, and every
/// run of anything else one `-`, none at either end. It may still start with
/// a digit or be empty, so a caller adds its own prefix where that can happen
/// (`site-12`), and checks with [`is_valid`]. Copies.
#[must_use]
pub fn slug(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.is_empty() && !out.ends_with('-') {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

/// `entities/{entity}`.
#[must_use]
pub fn entity_name(entity: &str) -> String {
    format!("entities/{entity}")
}

/// `entities/{entity}/signals/{signal}`.
#[must_use]
pub fn signal_name(entity: &str, signal: &str) -> String {
    format!("entities/{entity}/signals/{signal}")
}

/// `streams/{stream}`.
#[must_use]
pub fn stream_name(stream: &str) -> String {
    format!("streams/{stream}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_checked_exactly_as_the_daemon_checks_it() {
        assert!(is_valid("pump-01"));
        assert!(is_valid(&"a".repeat(MAX_ID)));
        assert!(!is_valid(&"a".repeat(MAX_ID + 1)));
        assert!(!is_valid(""));
        assert!(!is_valid("1pump"));
        assert!(!is_valid("Pump"));
        assert!(!is_valid("pump_01"));
    }

    #[test]
    fn a_slug_keeps_letters_and_digits_and_folds_the_rest() {
        assert_eq!(slug("Tower A / Basement"), "tower-a-basement");
        assert_eq!(slug("--x--"), "x");
        assert_eq!(slug("---"), "");
    }
}
