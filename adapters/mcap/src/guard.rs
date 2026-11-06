//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Checks a record's string lengths before the mcap crate reads them.
//!
//! `mcap::parse_record` reserves a string's stated length before reading it, so a
//! damaged 4-byte length in a record of a few bytes asks the allocator for up to
//! 4 GiB, which aborts the process on a small machine. Every length here must
//! lie inside the record, so a parse can only ever reserve what the record holds.

use mcap::records::op;

/// One field of a record, as the MCAP specification lays it out.
#[derive(Clone, Copy)]
enum Field {
    /// So many bytes of fixed-width integers.
    Fixed(usize),
    /// A `u32` length and that many bytes; also an integer map, read in a loop.
    Sized,
    /// A `u32` length in bytes of strings, in key and value pairs.
    Strings,
}

use Field::{Fixed, Sized, Strings};

/// The fields of each record holding a string, up to the last of them.
fn layout(opcode: u8) -> &'static [Field] {
    match opcode {
        op::HEADER => &[Sized, Sized],
        op::SCHEMA => &[Fixed(2), Sized, Sized],
        op::CHANNEL => &[Fixed(4), Sized, Sized, Strings],
        op::CHUNK => &[Fixed(28), Sized],
        op::CHUNK_INDEX => &[Fixed(32), Sized, Fixed(8), Sized],
        op::ATTACHMENT => &[Fixed(16), Sized, Sized],
        op::ATTACHMENT_INDEX => &[Fixed(40), Sized, Sized],
        op::METADATA => &[Sized, Strings],
        op::METADATA_INDEX => &[Fixed(16), Sized],
        _ => &[],
    }
}

/// The end of the `u32`-prefixed run at `at`, or `None` past the end of `body`.
fn sized(body: &[u8], at: usize) -> Option<usize> {
    let head = body.get(at..at.checked_add(4)?)?;
    let len = u32::from_le_bytes(head.try_into().ok()?);
    let end = (at + 4).checked_add(usize::try_from(len).ok()?)?;
    (end <= body.len()).then_some(end)
}

/// The end of the string map at `at`.
fn strings(body: &[u8], at: usize) -> Option<usize> {
    let end = sized(body, at)?;
    let mut next = at + 4;
    while next < end {
        next = sized(body, sized(body, next)?)?;
    }
    Some(end)
}

/// Whether every length in this record lies inside it. Borrows; allocates nothing.
pub(crate) fn fits(opcode: u8, body: &[u8]) -> bool {
    let mut at = Some(0usize);
    for field in layout(opcode) {
        at = at.and_then(|at| match field {
            Fixed(n) => at.checked_add(*n),
            Sized => sized(body, at),
            Strings => strings(body, at),
        });
    }
    at.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(text: &str) -> Vec<u8> {
        let mut out = (text.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(text.as_bytes());
        out
    }

    #[test]
    fn a_record_whose_lengths_lie_inside_it_fits() {
        let body = [string("profile"), string("library")].concat();
        assert!(fits(op::HEADER, &body));
    }

    #[test]
    fn a_string_longer_than_its_record_does_not() {
        let mut body = string("profile");
        body.extend_from_slice(&0x6e6e_0000u32.to_le_bytes());
        assert!(!fits(op::HEADER, &body));
    }

    #[test]
    fn a_string_in_a_map_longer_than_its_record_does_not() {
        let entry = [string("key"), 0x7000_0000u32.to_le_bytes().to_vec()].concat();
        let mut body = string("norn.catalog");
        body.extend_from_slice(&(entry.len() as u32).to_le_bytes());
        body.extend_from_slice(&entry);
        assert!(!fits(op::METADATA, &body));
    }

    #[test]
    fn a_record_with_no_strings_always_fits() {
        assert!(fits(op::MESSAGE, &[]));
        assert!(fits(op::DATA_END, &[1, 2]));
    }
}
