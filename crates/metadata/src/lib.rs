//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's durable registry of entities, signals and streams (SPEC §10, §11).
//!
//! # Layer contract
//!
//! - **Owns**: the [`Registry`]: the resource records and their limits
//!   (`Config::limits`, `Config::metadata`), the mapping from client-chosen
//!   resource IDs to compact core IDs ([`norn_core::SignalId`] and friends,
//!   allocated here and never reused), and the checksummed append log that
//!   makes them durable and recoverable.
//! - **Must not**: read a clock (times are handed in), know the wire protocol,
//!   know any domain (annotations are opaque strings), or touch samples. It
//!   learns that a signal holds data from whoever stores it.
//! - **May import**: `norn-core` only. No third-party crate.
//! - **Spans**: none; recovery findings are returned as a [`LogRecovery`].
//!
//! # Concurrency
//!
//! A [`Registry`] is `Send + Sync`. Reads take a shared lock and never touch
//! the disk. A mutation blocks on the disk: call it from a blocking context.
//! Concurrent mutations share one fsync (group commit), and each returns only
//! once its change is durable. Two processes cannot open one directory: the
//! second gets [`MetadataError::Locked`].
//!
//! # Failure
//!
//! A torn tail is an unacknowledged mutation: it is kept in a side file, cut,
//! and reported. Damage anywhere else refuses to open with
//! [`MetadataError::Corrupt`], because replaying around it would silently undo
//! acknowledged changes (SPEC §20). A failed write or fsync poisons the registry
//! until restart, since memory and disk may then disagree.
//!
//! # Memory
//!
//! Every record stays in memory, bounded by `max_entities`, `max_signals` and
//! `max_streams` times the metadata limits. The log on disk is rewritten once
//! it has more than twice the live records plus 1,024, so it stays bounded too.

#![forbid(unsafe_code)]

mod codec;
mod crc;
mod error;
mod log;
mod op;
mod record;
mod registry;
mod resource;

pub use error::{MetadataError, ResourceKind};
pub use log::LogRecovery;
pub use record::{EntityRecord, Retention, SignalRecord, StreamRecord};
pub use registry::{
    EntityPatch, EntitySpec, MAX_BATCH_CREATE, Registry, RegistryView, SignalPatch, SignalRoute,
    SignalSpec, StreamPatch, StreamSpec, Write,
};
pub use resource::{MAX_DISPLAY_NAME_CHARS, check_display_name, check_resource_id};
