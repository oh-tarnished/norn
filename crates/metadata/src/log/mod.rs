//! The append log: every mutation, checksummed, then rewritten when mostly dead.
//!
//! ```text
//! metadata.log:  "NORNMETA" | version u32 | CRC-32C of those 12 bytes
//!                then records: length u32 | CRC-32C of payload u32 | payload
//! ```
//!
//! Chosen over rewriting a snapshot per change because a site creates 16,000
//! signals at once: a rewrite per create would be quadratic in bytes and fsyncs.

mod replay;

use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::crc::crc32c;
use crate::error::MetadataError;
use crate::op::Op;

const MAGIC: &[u8; 8] = b"NORNMETA";
const VERSION: u32 = 1;
const HEADER_LEN: usize = 16;
const RECORD_HEADER_LEN: usize = 8;

/// The largest record accepted, checked before anything is allocated. A signal
/// with default limits is about 75 KiB; this leaves room for raised limits.
const MAX_RECORD_BYTES: u32 = 16 * 1024 * 1024;

/// What opening the log found.
///
/// Concurrency: owned plain data.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct LogRecovery {
    /// Records replayed.
    pub records: u64,
    /// Bytes of a torn tail: a mutation never acknowledged, cut from the log.
    pub torn_bytes: u64,
    /// Where those bytes were kept, so nothing is silently discarded.
    pub torn_to: Option<PathBuf>,
}

/// The open log. Not synchronised: the registry owns the locking.
#[derive(Debug)]
pub(crate) struct Log {
    pub(crate) path: PathBuf,
    file: File,
    pub(crate) records: u64,
}

fn io(op: &'static str, path: &Path) -> impl FnOnce(std::io::Error) -> MetadataError {
    let path = path.to_path_buf();
    move |source| MetadataError::Io { op, path, source }
}

fn frame(op: &Op, out: &mut Vec<u8>) {
    let payload = op.encode();
    let len = u32::try_from(payload.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_le_bytes());
    out.extend_from_slice(&crc32c(&payload).to_le_bytes());
    out.extend_from_slice(&payload);
}

impl Log {
    /// Opens or creates the log at `path`, calling `apply` on each record in
    /// order, and cuts a torn tail (keeping it in a side file).
    pub(crate) fn open(
        path: &Path,
        apply: impl FnMut(Op),
    ) -> Result<(Self, LogRecovery), MetadataError> {
        let tmp = path.with_extension("log.tmp");
        if tmp.exists() {
            fs::remove_file(&tmp).map_err(io("remove", &tmp))?;
        }
        if !path.exists() {
            let mut f = File::create(&tmp).map_err(io("create", &tmp))?;
            f.write_all(&replay::header()).map_err(io("write", &tmp))?;
            f.sync_all().map_err(io("fsync", &tmp))?;
            fs::rename(&tmp, path).map_err(io("rename", path))?;
            sync_parent(path)?;
        }
        let mut bytes = Vec::new();
        File::open(path)
            .and_then(|mut f| f.read_to_end(&mut bytes))
            .map_err(io("read", path))?;
        let (end, records) = replay::scan(&bytes, path, apply)?;
        let mut report = LogRecovery {
            records,
            ..LogRecovery::default()
        };
        let tail = bytes.get(end..).unwrap_or_default();
        if !tail.is_empty() {
            let side = path.with_extension(format!("log.torn-{end}"));
            let mut f = File::create(&side).map_err(io("create", &side))?;
            f.write_all(tail).map_err(io("write", &side))?;
            f.sync_all().map_err(io("fsync", &side))?;
            sync_parent(&side)?;
            report.torn_bytes = tail.len() as u64;
            report.torn_to = Some(side);
        }
        let mut file = OpenOptions::new()
            .write(true)
            .open(path)
            .map_err(io("open", path))?;
        file.set_len(end as u64).map_err(io("truncate", path))?;
        file.seek(SeekFrom::End(0)).map_err(io("seek", path))?;
        file.sync_all().map_err(io("fsync", path))?;
        let log = Self {
            path: path.to_path_buf(),
            file,
            records,
        };
        Ok((log, report))
    }

    /// Writes one record to the OS. Not yet durable: the registry fsyncs.
    pub(crate) fn append(&mut self, op: &Op) -> Result<(), MetadataError> {
        let mut buf = Vec::new();
        frame(op, &mut buf);
        self.file.write_all(&buf).map_err(io("write", &self.path))?;
        self.records += 1;
        Ok(())
    }

    /// A second handle on the file, so an fsync never holds up appends.
    pub(crate) fn sync_handle(&self) -> Result<File, MetadataError> {
        self.file.try_clone().map_err(io("open", &self.path))
    }

    /// Replaces the log with `ops`: write beside it, fsync, rename, fsync the
    /// directory. A crash leaves the old log or the new one, never a mix.
    pub(crate) fn rewrite(&mut self, ops: &[Op]) -> Result<(), MetadataError> {
        let tmp = self.path.with_extension("log.tmp");
        let f = File::create(&tmp).map_err(io("create", &tmp))?;
        let mut out = BufWriter::new(f);
        out.write_all(&replay::header())
            .map_err(io("write", &tmp))?;
        let mut buf = Vec::new();
        for op in ops {
            buf.clear();
            frame(op, &mut buf);
            out.write_all(&buf).map_err(io("write", &tmp))?;
        }
        let f = out
            .into_inner()
            .map_err(|e| io("write", &tmp)(e.into_error()))?;
        f.sync_all().map_err(io("fsync", &tmp))?;
        drop(f);
        fs::rename(&tmp, &self.path).map_err(io("rename", &self.path))?;
        sync_parent(&self.path)?;
        let mut file = OpenOptions::new()
            .write(true)
            .open(&self.path)
            .map_err(io("open", &self.path))?;
        file.seek(SeekFrom::End(0))
            .map_err(io("seek", &self.path))?;
        self.file = file;
        self.records = ops.len() as u64;
        Ok(())
    }
}

/// Makes a new or renamed file's name durable. Windows cannot open a directory
/// with `std` and NTFS journals metadata, so failing to open it is not an error,
/// as in norn-storage.
fn sync_parent(path: &Path) -> Result<(), MetadataError> {
    let Some(dir) = path.parent() else {
        return Ok(());
    };
    let Ok(d) = File::open(dir) else {
        return Ok(());
    };
    match d.sync_all() {
        Err(e) if e.kind() != std::io::ErrorKind::PermissionDenied => Err(io("fsync", dir)(e)),
        _ => Ok(()),
    }
}
