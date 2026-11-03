//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The gRPC metadata of the shared-memory data plane (protocol 1.7).
//!
//! The schema says how to reach the plane (`admin::SharedMemoryPlane`, in
//! `NegotiateProtocolResponse.shared_memory`); these two keys ask for and grant
//! one ring on one `AppendSamples` call. Metadata, because a bidirectional
//! stream has no message the daemon can answer before the client's first batch,
//! and a ring must be granted before that batch is written into it.

/// The request metadata key a client sets on `AppendSamples` to ask for a ring,
/// valued [`DATA_PLANE_SHARED_MEMORY`]. A daemon that cannot grant one ignores
/// it, and the call carries its batches over gRPC.
pub const DATA_PLANE_METADATA_KEY: &str = "norn-data-plane";

/// The only value of [`DATA_PLANE_METADATA_KEY`] defined so far.
pub const DATA_PLANE_SHARED_MEMORY: &str = "shared-memory";

/// The response metadata key with which a daemon grants a ring: the ring's whole
/// iceoryx2 service name, `{SharedMemoryPlane.service}/{ring}`, printable ASCII.
/// Absent, the call is on gRPC.
pub const SHARED_MEMORY_SERVICE_METADATA_KEY: &str = "norn-shared-memory-service";

/// The first minor version of major 1 that carries the plane's description.
pub const SHARED_MEMORY_MINOR: i32 = 7;
