//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Heading anchors, computed as GitHub computes them.

use std::collections::BTreeSet;

use super::markdown::prose;

/// Every anchor `text` offers: heading slugs as GitHub makes them, with `-1`,
/// `-2` for repeats, and explicit `id` and `name` attributes.
#[must_use]
pub fn anchors(text: &str) -> BTreeSet<String> {
    let mut found = BTreeSet::new();
    for line in prose(text) {
        if let Some(heading) = heading(&line) {
            let base = slug(heading);
            let mut anchor = base.clone();
            let mut repeat = 0;
            while !found.insert(anchor.clone()) {
                repeat += 1;
                anchor = format!("{base}-{repeat}");
            }
        }
        for attribute in [" id=\"", " name=\""] {
            for (at, _) in line.match_indices(attribute) {
                let rest = line.get(at + attribute.len()..).unwrap_or("");
                if let Some((id, _)) = rest.split_once('"') {
                    found.insert(id.to_owned());
                }
            }
        }
    }
    found
}

/// The text of an ATX heading of any level.
#[must_use]
pub fn heading(line: &str) -> Option<&str> {
    let bare = line.trim_start_matches(' ');
    let text = bare.trim_start_matches('#');
    let level = bare.len() - text.len();
    let text = text.strip_prefix(' ')?;
    (1..=6)
        .contains(&level)
        .then(|| text.trim().trim_end_matches('#').trim_end())
}

/// GitHub's slug: link targets go, then lowercase, punctuation dropped, each
/// space a hyphen. Runs are not collapsed: `a & b` is `a--b`.
#[must_use]
pub fn slug(heading: &str) -> String {
    let mut text = String::with_capacity(heading.len());
    let mut rest = heading;
    while let Some((before, after)) = rest.split_once("](") {
        text.push_str(before);
        rest = after.split_once(')').map_or("", |(_, tail)| tail);
    }
    text.push_str(rest);
    text.trim()
        .chars()
        .flat_map(char::to_lowercase)
        .filter(|c| c.is_alphanumeric() || matches!(c, '-' | '_' | ' '))
        .map(|c| if c == ' ' { '-' } else { c })
        .collect()
}
