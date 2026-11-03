//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Quality codes, timestamps and sequences.

use norn_core::{Error, Quality, Sequence, Severity, Substatus, Timestamp};

#[test]
fn every_valid_quality_code_round_trips() {
    let mut seen = 0;
    for code in 0..=u16::MAX {
        if let Ok(q) = Quality::from_code(code) {
            assert_eq!(q.code(), code);
            seen += 1;
        }
    }
    assert_eq!(seen, 3 * 12, "three severities times twelve substatuses");
}

#[test]
fn a_quality_code_outside_the_closed_set_is_refused() {
    assert_eq!(
        Quality::from_code(0x0300),
        Err(Error::InvalidQuality { code: 0x0300 })
    );
    assert_eq!(
        Quality::from_code(0x000c),
        Err(Error::InvalidQuality { code: 0x000c })
    );
}

#[test]
fn the_default_quality_is_good_with_no_substatus() {
    assert_eq!(Quality::default(), Quality::GOOD);
    assert_eq!(Quality::GOOD.code(), 0);
    let q = Quality::new(Severity::Bad, Substatus::CommunicationFailure);
    assert_eq!(q.code(), 0x0207);
}

#[test]
fn timestamps_refuse_to_overflow_instead_of_wrapping() {
    assert_eq!(Timestamp::MAX.checked_add_nanos(1), Err(Error::Overflow));
    assert_eq!(Timestamp::from_secs(i64::MAX), Err(Error::Overflow));
    assert_eq!(
        Timestamp::MIN.nanos_since(Timestamp::MAX),
        Err(Error::Overflow)
    );
}

#[test]
fn timestamps_count_nanoseconds_from_the_unix_epoch() {
    let t = Timestamp::from_secs(1).unwrap();
    assert_eq!(t.as_nanos(), 1_000_000_000);
    assert_eq!(t.nanos_since(Timestamp::UNIX_EPOCH), Ok(1_000_000_000));
    assert_eq!(t.checked_add_nanos(-1).unwrap().as_nanos(), 999_999_999);
}

#[test]
fn a_sequence_refuses_to_wrap() {
    assert_eq!(Sequence::ZERO.next().unwrap().get(), 1);
    assert_eq!(Sequence::new(u64::MAX).next(), Err(Error::Overflow));
}
