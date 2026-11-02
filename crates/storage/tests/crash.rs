//! Crash tests: a file cut anywhere recovers to its complete chunks.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use std::fs;

use common::{Scratch, bytes_of, chunk, read_all, sealed, unsealed};
use norn_storage::format::{HEADER_LEN, TRAILER_LEN};
use norn_storage::{
    Action, CorruptionKind, RecoveryEvent, RecoveryMode, SegmentReader, StorageError,
    StorageOptions, recover_segment, recover_stream,
};

fn opts() -> StorageOptions {
    StorageOptions::default()
}

#[test]
fn truncating_at_every_byte_of_the_last_chunk_keeps_every_complete_chunk() {
    let dir = Scratch::new();
    let chunks = [chunk(1, 0, 40), chunk(2, 0, 40), chunk(3, 0, 40)];
    let path = unsealed(dir.path(), 0, &chunks);
    let full = fs::read(&path).unwrap();
    let last = SegmentReader::open(&path, opts()).unwrap().chunks()[2];
    let start = usize::try_from(last.offset).unwrap();
    let want = bytes_of(
        &chunks[..2]
            .iter()
            .flat_map(|c| c.samples())
            .collect::<Vec<_>>(),
    );
    assert!(full.len() > start, "the sweep must examine bytes");
    let mut examined = 0;
    for cut in start..full.len() {
        let case = Scratch::new();
        let torn = case.path().join(path.file_name().unwrap());
        fs::write(&torn, &full[..cut]).unwrap();

        let report = recover_segment(&torn, &opts(), RecoveryMode::Repair).unwrap();
        let summary = report.summary.as_ref().unwrap();
        assert_eq!(summary.chunks, 2, "cut at {cut}");
        assert_eq!(bytes_of(&read_all(&torn)), want, "cut at {cut}");
        assert!(SegmentReader::open(&torn, opts()).unwrap().is_sealed());

        let torn_event = report.events.iter().find(|e| e.is_corruption());
        if cut == start {
            assert!(torn_event.is_none(), "nothing was torn at a frame boundary");
        } else {
            let Some(RecoveryEvent::Corruption { corruption, action }) = torn_event else {
                panic!(
                    "cut at {cut}: the torn chunk was not reported: {:?}",
                    report.events
                );
            };
            assert_eq!(corruption.kind, CorruptionKind::TornTail, "cut at {cut}");
            assert_eq!(corruption.offset, last.offset);
            assert_eq!(corruption.len, (cut - start) as u64);
            let Action::QuarantinedAndTruncated { to } = action else {
                panic!("cut at {cut}: {action:?}");
            };
            assert_eq!(fs::read(to).unwrap(), &full[start..cut], "bytes kept aside");
        }
        // Idempotent: nothing left to find.
        let again = recover_segment(&torn, &opts(), RecoveryMode::Repair).unwrap();
        assert!(again.is_clean(), "cut at {cut}: {:?}", again.events);
        examined += 1;
    }
    assert_eq!(examined, full.len() - start);
}

#[test]
fn a_crash_at_every_byte_of_the_seal_loses_no_chunk() {
    let dir = Scratch::new();
    let chunks = [chunk(1, 0, 30), chunk(2, 0, 30)];
    let seg = sealed(dir.path(), 0, &chunks);
    let full = fs::read(&seg.path).unwrap();
    let data_end = SegmentReader::open(&seg.path, opts()).unwrap().chunks()[1];
    let data_end = usize::try_from(data_end.offset + data_end.frame_len()).unwrap();
    assert_eq!(full.len() - data_end, 8 + 2 * 44 + 4 + TRAILER_LEN);
    for cut in data_end..full.len() {
        let case = Scratch::new();
        let torn = case.path().join(seg.path.file_name().unwrap());
        fs::write(&torn, &full[..cut]).unwrap();
        let report = recover_segment(&torn, &opts(), RecoveryMode::Repair).unwrap();
        assert_eq!(report.summary.as_ref().unwrap().chunks, 2, "cut at {cut}");
        assert_eq!(read_all(&torn).len(), 60, "cut at {cut}");
        assert!(SegmentReader::open(&torn, opts()).unwrap().is_sealed());
        // The rebuilt seal is byte-identical to the one that was torn.
        assert_eq!(fs::read(&torn).unwrap(), full, "cut at {cut}");
    }
}

#[test]
fn a_cut_inside_the_header_quarantines_the_whole_file_and_reports_it() {
    for cut in 0..HEADER_LEN {
        let dir = Scratch::new();
        let path = unsealed(dir.path(), 0, &[chunk(1, 0, 3)]);
        let bytes = fs::read(&path).unwrap();
        fs::write(&path, &bytes[..cut]).unwrap();
        let r = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap();
        assert!(r.segments.is_empty());
        let [
            RecoveryEvent::SegmentQuarantined {
                corruption,
                to: Some(to),
            },
        ] = &r.events[..]
        else {
            panic!("cut at {cut}: {:?}", r.events);
        };
        assert_eq!(corruption.kind, CorruptionKind::TornTail);
        assert_eq!(fs::read(to).unwrap(), &bytes[..cut]);
        assert!(!path.exists());
    }
}

#[test]
fn report_only_recovery_finds_the_same_tear_and_changes_nothing() {
    let dir = Scratch::new();
    let path = unsealed(dir.path(), 0, &[chunk(1, 0, 20), chunk(2, 0, 20)]);
    let full = fs::read(&path).unwrap();
    fs::write(&path, &full[..full.len() - 3]).unwrap();
    let before = fs::read(&path).unwrap();
    let r = recover_stream(dir.path(), &opts(), RecoveryMode::ReportOnly).unwrap();
    assert_eq!(r.corruptions().count(), 1);
    assert!(matches!(
        &r.events[0],
        RecoveryEvent::Corruption {
            action: Action::Reported,
            ..
        }
    ));
    assert_eq!(fs::read(&path).unwrap(), before);
    assert!(!dir.path().join(norn_storage::QUARANTINE_DIR).exists());
}

#[test]
fn a_recurring_sequence_number_never_overwrites_earlier_quarantined_bytes() {
    let dir = Scratch::new();
    let mut kept = Vec::new();
    for fill in [1u8, 2u8] {
        let path = dir.path().join("0000000000000000-g0000.nseg");
        fs::write(&path, [fill; 10]).unwrap();
        let r = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap();
        let [RecoveryEvent::SegmentQuarantined { to: Some(to), .. }] = &r.events[..] else {
            panic!("{:?}", r.events);
        };
        kept.push(to.clone());
    }
    assert_ne!(kept[0], kept[1]);
    assert_eq!(fs::read(&kept[0]).unwrap(), [1u8; 10]);
    assert_eq!(fs::read(&kept[1]).unwrap(), [2u8; 10]);
}

#[test]
fn an_unsealed_segment_with_a_torn_tail_is_not_an_error_for_a_reader() {
    let dir = Scratch::new();
    let path = unsealed(dir.path(), 0, &[chunk(1, 0, 20), chunk(2, 0, 20)]);
    let full = fs::read(&path).unwrap();
    fs::write(&path, &full[..full.len() - 1]).unwrap();
    let r = SegmentReader::open(&path, opts()).unwrap();
    assert_eq!(r.chunks().len(), 1);
    assert!(r.unverified_tail().is_some());
}

#[test]
fn a_file_from_a_newer_norn_is_refused_and_left_untouched() {
    let dir = Scratch::new();
    let seg = sealed(dir.path(), 0, &[chunk(1, 0, 3)]);
    let mut b = fs::read(&seg.path).unwrap();
    b[8] = 9; // format_version
    let crc = norn_storage::crc32c(&b[..44]);
    b[44..48].copy_from_slice(&crc.to_le_bytes());
    fs::write(&seg.path, &b).unwrap();
    let e = recover_stream(dir.path(), &opts(), RecoveryMode::Repair).unwrap_err();
    assert!(matches!(
        e,
        StorageError::UnsupportedVersion { found: 9, .. }
    ));
    assert_eq!(fs::read(&seg.path).unwrap(), b);
}
