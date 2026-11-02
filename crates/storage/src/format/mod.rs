//! The segment file's byte layout: constants and the three fixed records.
//!
//! A segment file is, in order (docs/reference/storage.md "Segment file" has the table):
//!
//! 1. a [`SegmentHeader`] of [`HEADER_LEN`] bytes;
//! 2. any number of chunk frames, each a [`FRAME_HEADER_LEN`]-byte header
//!    (see [`frame`]) and a body of four sealed column blocks;
//! 3. once sealed, a chunk directory and a [`TRAILER_LEN`]-byte trailer
//!    (see [`directory`]).
//!
//! All integers are little-endian. Every record carries its own CRC-32C, so
//! a reader trusts no byte it has not checked. Layout changes bump
//! [`FORMAT_VERSION`]; changes to the chunk's columns bump
//! [`SCHEMA_VERSION`]; a reader refuses versions newer than its own.

pub mod directory;
pub mod frame;
mod header;
mod le;

pub use header::SegmentHeader;
pub(crate) use le::Le;

/// First eight bytes of every segment file.
pub const SEGMENT_MAGIC: [u8; 8] = *b"NORNSEG\0";
/// Version of the container layout described in this module.
pub const FORMAT_VERSION: u16 = 1;
/// Version of the chunk column schema (timestamps, values, quality,
/// sequence, in that order; value kinds by their `norn_core` tag).
pub const SCHEMA_VERSION: u16 = 1;
/// Bytes in a [`SegmentHeader`].
pub const HEADER_LEN: usize = 48;
/// First four bytes of every chunk frame: the recovery scanner's sync mark.
pub const FRAME_MAGIC: [u8; 4] = *b"NCHK";
/// Bytes in a chunk frame header.
pub const FRAME_HEADER_LEN: usize = 48;
/// First four bytes of the chunk directory.
pub const DIR_MAGIC: [u8; 4] = *b"NDIR";
/// Bytes per chunk directory entry.
pub const DIR_ENTRY_LEN: usize = 44;
/// Last eight bytes of a sealed segment: the seal marker.
pub const TRAILER_MAGIC: [u8; 8] = *b"NORNEND\0";
/// Bytes in the trailer: directory offset (u64) and [`TRAILER_MAGIC`].
pub const TRAILER_LEN: usize = 16;
