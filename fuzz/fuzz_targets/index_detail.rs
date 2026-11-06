//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `*.ndet` sidecars: a head and pages parse or send the caller back to the
//! segment, whatever the record claims, and what parses encodes back to itself.

#![no_main]

use libfuzzer_sys::fuzz_target;
use norn_fuzz::segment::STREAM;
use norn_fuzz::{MAX_INPUT, measure, split, with_crc};
use norn_index::fuzzing::{
    SUMMARY_RECORD_LEN, decode_detail, decode_detail_page, decode_record, detail_head_len,
    encode_detail_file,
};

fuzz_target!(|data: &[u8]| {
    if data.len() > MAX_INPUT {
        return;
    }
    let Some((selector, rest)) = split(data) else {
        return;
    };
    let Some((head, file)) = rest.split_at_checked(SUMMARY_RECORD_LEN) else {
        return;
    };
    let Some(record) = decode_record(head) else {
        return;
    };
    // A page behind a good CRC, with a claim of its place in the file.
    let page = usize::from(selector >> 1);
    let (decoded, peak) = measure(|| decode_detail_page(&with_crc(file), record.chunks, page));
    let limit = 64 * file.len() + (1 << 20);
    assert!(peak <= limit, "a page held {peak} bytes, above {limit}");
    drop(decoded);
    // The file as given, or with its head's CRC repaired when it has a whole head.
    let mut whole = file.to_vec();
    let head_len = detail_head_len(record.chunks);
    if selector & 1 == 1 && head_len <= whole.len() && head_len >= 4 {
        let fixed = with_crc(&whole[..head_len - 4]);
        whole[..head_len].copy_from_slice(&fixed);
    }
    let (metas, peak) = measure(|| decode_detail(&whole, STREAM, &record));
    assert!(peak <= limit, "a sidecar held {peak} bytes, above {limit}");
    if let Some(mut metas) = metas {
        let encoded = encode_detail_file(STREAM, &record, &mut metas);
        assert_eq!(
            decode_detail(&encoded, STREAM, &record).as_ref(),
            Some(&metas)
        );
    }
});
