//! Unix: open the directory read-only and fsync it.

use std::fs::File;
use std::io;
use std::path::Path;

pub(super) fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}
