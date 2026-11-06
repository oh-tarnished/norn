//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Checks an Arrow file's footer before `arrow-ipc` trusts it.
//!
//! The reader allocates a block's stated length before reading it, and unwraps
//! it into a `usize`, so a damaged footer asks for an exabyte or panics. Every
//! block must lie inside the file, so a read can only reserve what the file holds.

use std::io::{Read, Seek, SeekFrom};

use arrow_ipc::Block;
use arrow_ipc::reader::read_footer_length;
use arrow_ipc::root_as_footer;

/// Bytes after the footer: its length and the magic.
const TAIL: u64 = 10;

fn inside(block: &Block, file: u64) -> bool {
    let wide = |n: i64| u64::try_from(n).ok();
    let end = wide(block.offset()).and_then(|offset| {
        offset
            .checked_add(wide(i64::from(block.metaDataLength()))?)?
            .checked_add(wide(block.bodyLength())?)
    });
    end.is_some_and(|end| end <= file)
}

/// Whether every block in the footer of `input` lies inside it. Leaves `input` at
/// its start. Holds the footer, which is no longer than the file, while it looks.
///
/// # Errors
/// What is wrong, in words: the caller reports it as the file's damage.
pub(crate) fn check<R: Read + Seek>(input: &mut R) -> Result<(), String> {
    let text = |e: &dyn std::fmt::Display| e.to_string();
    let file = input.seek(SeekFrom::End(0)).map_err(|e| text(&e))?;
    let body = file.checked_sub(TAIL).ok_or("shorter than a footer")?;
    input.seek(SeekFrom::Start(body)).map_err(|e| text(&e))?;
    let mut tail = [0; TAIL as usize];
    input.read_exact(&mut tail).map_err(|e| text(&e))?;
    let len = read_footer_length(tail).map_err(|e| text(&e))? as u64;
    let start = body
        .checked_sub(len)
        .ok_or("a footer longer than the file")?;
    input.seek(SeekFrom::Start(start)).map_err(|e| text(&e))?;
    let mut footer = vec![0; usize::try_from(len).map_err(|e| text(&e))?];
    input.read_exact(&mut footer).map_err(|e| text(&e))?;
    let footer = root_as_footer(&footer).map_err(|e| text(&e))?;
    let batches = footer.recordBatches().into_iter().flatten();
    let dictionaries = footer.dictionaries().into_iter().flatten();
    if !batches.chain(dictionaries).all(|b| inside(b, file)) {
        return Err("a block past the end of the file".to_owned());
    }
    input.seek(SeekFrom::Start(0)).map_err(|e| text(&e))?;
    Ok(())
}
