//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The only place this crate selects code by OS (SPEC §5).
//!
//! Each platform module exports the same five functions with the same
//! signatures — `bind_unix`, `connect_unix`, `bind_pipe`, `connect_pipe`,
//! `default_endpoint` — and answers [`TransportError::Unsupported`] for the
//! transport its OS lacks. Everything above calls them through `os::`, so
//! no other file carries a `cfg`.
//!
//! [`TransportError::Unsupported`]: crate::TransportError::Unsupported

#[cfg(unix)]
pub(crate) mod unix;
#[cfg(unix)]
pub(crate) use unix as os;

#[cfg(windows)]
pub(crate) mod windows;
#[cfg(windows)]
pub(crate) use windows as os;

pub(crate) use os::default_endpoint;
