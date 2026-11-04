//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Norn's shared-memory data plane over iceoryx2 (SPEC §7, stage 12).
//!
//! # Layer contract
//!
//! - **Owns**: [`Settings`], the daemon's `[shared_memory]` offer, and what a
//!   ring costs; on Unix, `Ring`, the daemon's end of one bounded iceoryx2
//!   service; `Producer`, the client's end, which implements
//!   [`norn_ipc::DataPlane`]; and `SharedMemory`, which implements
//!   [`norn_ipc::Attach`] so that `norn_ipc::AppendCall` can use a ring when one
//!   is granted. Off by default, everywhere. On Windows iceoryx2 is not built
//!   at all, since a Windows daemon never offers a ring (`platform/windows.rs`
//!   says why): `Ring` and `SharedMemory` are stand-ins that refuse, and there
//!   is no `Producer`.
//! - **Must not**: decide anything about a batch. Admission, budgets,
//!   backpressure and acknowledgement stay in the daemon's `AppendSamples`
//!   handler, and every response travels on the gRPC call that was granted the
//!   ring, so a batch gets the same answers on either plane. It does not touch
//!   storage, and adds no `unsafe` of its own.
//! - **May import**: `norn-ipc`; third-party `tokio` and, on Unix only,
//!   `iceoryx2` (C-free on every target Norn builds: xtask/src/iceoryx2.rs).
//! - **Spans**: none. iceoryx2's own logger is silenced: it writes to stderr,
//!   unbounded.
//!
//! **Unsafe:** none: `#![forbid(unsafe_code)]`. iceoryx2's own `unsafe` is not
//! ours to forbid, and it trusts its peer: docs/reference/shared-memory.md says
//! what a misbehaving process of the same user can do to a daemon with shared
//! memory enabled, which is why it is opt-in.
//!
//! Concurrency: `Producer` and [`SharedMemory`] are `Send + Sync`; a [`Ring`]
//! is `Send + Sync` but meant to be emptied by one thread.
#![forbid(unsafe_code)]

mod error;
mod platform;
#[cfg(unix)]
mod rings;
mod settings;
mod sizing;

pub use error::PlaneError;
pub use norn_ipc::SharedMemoryTicket;
pub use platform::{OWNER_ONLY_MAPPINGS, Ring, SharedMemory, create_root, ring_root};
#[cfg(unix)]
pub use rings::Producer;
pub use settings::Settings;
pub use sizing::{chunks_per_ring, ring_bytes, ring_prefix};
