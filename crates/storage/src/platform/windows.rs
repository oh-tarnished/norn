//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Windows: no directory fsync through `std`; NTFS journals metadata.

use std::io;
use std::path::Path;

pub(super) fn sync_dir(_dir: &Path) -> io::Result<()> {
    Ok(())
}
