//! `refresh` beside a live writer: what it may and may not read.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{STREAM, Scratch, chunk, open, sealed, writer};
use norn_index::PlanRequest;
use norn_storage::format::HEADER_LEN;

/// A writer creates its segment and buffers the header, so for a moment the file
/// is empty. A query that met a compaction refreshes the index at exactly such a
/// moment: it found the daemon's next segment empty and failed with `TornTail`.
#[test]
fn a_refresh_beside_a_writer_whose_header_is_not_on_disk_yet_skips_that_file() {
    let s = Scratch::new();
    sealed(&s.dir(), 0, &[chunk(1, 0, 10, 1)]);
    let index = open(&s);
    let mut open_segment = writer(&s.dir(), 1);
    open_segment.append(&chunk(1, 100, 10, 1)).unwrap();
    let path = s.dir().join("0000000000000001-g0000.nseg");
    let on_disk = std::fs::metadata(&path).unwrap().len();
    assert!(
        on_disk < HEADER_LEN as u64,
        "the header reached the disk: {on_disk}"
    );

    let report = index.refresh(STREAM).unwrap();
    assert_eq!(report.read, 0, "a file with no header was read");
    let snapshot = index.snapshot();
    assert_eq!(snapshot.segments(STREAM).len(), 1);
    assert_eq!(snapshot.tail_chunks(STREAM), 0);
    let all = snapshot.plan(PlanRequest::new(common::secs(-1), common::secs(1_000)));
    assert_eq!(all.count(), 1);

    open_segment.flush().unwrap();
    index.refresh(STREAM).unwrap();
    assert_eq!(
        index.snapshot().tail_chunks(STREAM),
        1,
        "once written, it is seen"
    );
}
