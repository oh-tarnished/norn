//! The README holds no emoji or other pictographs.
//!
//! The owner's rule: the README of an infrastructure project reads as plain
//! text. A roadmap says Done, In progress or Planned in words, which a screen
//! reader, a terminal and `grep` all agree on; a check mark says nothing to any
//! of them. Typographic symbols (`©`, `×`, `µ`, `→`, dashes) are not pictographs
//! and stay allowed.

use crate::scan::Findings;

/// Unicode ranges whose characters render as pictographs: the emoji blocks,
/// dingbats (check and cross marks), miscellaneous symbols, the clock and
/// media symbols of Miscellaneous Technical, the emoji squares and stars of
/// Miscellaneous Symbols and Arrows, and the joiners that build sequences.
const PICTOGRAPHS: &[(u32, u32)] = &[
    (0x1F000, 0x1FAFF), // mahjong to symbols and pictographs extended-A, flags
    (0x2600, 0x27BF),   // miscellaneous symbols, dingbats
    (0x231A, 0x231B),   // watch, hourglass
    (0x23E9, 0x23FA),   // media controls, alarm clock, stopwatch
    (0x2B05, 0x2B07),   // heavy arrows
    (0x2B1B, 0x2B1C),   // large squares
    (0x2B50, 0x2B55),   // star, circle
    (0xFE0F, 0xFE0F),   // emoji presentation selector
    (0x200D, 0x200D),   // zero width joiner
    (0x20E3, 0x20E3),   // combining keycap
];

/// True when `c` is drawn as a pictograph rather than read as text.
#[must_use]
pub fn is_pictograph(c: char) -> bool {
    let code = u32::from(c);
    PICTOGRAPHS
        .iter()
        .any(|(low, high)| (*low..=*high).contains(&code))
}

/// Objects to every pictograph in `readme`, by line and code point. `None`, a
/// missing README, is an objection. `examined` counts lines, so an empty
/// README fails rather than passes.
#[must_use]
pub fn check(readme: Option<&str>) -> Findings {
    let mut findings = Findings::new("docs emoji");
    let Some(text) = readme else {
        findings.object("README.md", "is missing, so it cannot be checked");
        return findings;
    };
    for (index, line) in text.lines().enumerate() {
        findings.examined += 1;
        for c in line.chars().filter(|c| is_pictograph(*c)) {
            let subject = format!("README.md:{}", index + 1);
            let detail = format!("holds the pictograph U+{:04X}; use words", u32::from(c));
            findings.object(subject, detail);
        }
    }
    findings
}
