//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The protocol version this crate speaks (SPEC section 40).

/// A protocol version as plain numbers, so it can be compared without
/// building a message.
///
/// `major` equals the `vN` of every `norn.*` package and changes only with
/// a new package version. `minor` counts compatible additions within it.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProtocolVersion {
    /// The package version: 1 for `norn.*.v1`.
    pub major: i32,
    /// Compatible additions within `major`.
    pub minor: i32,
}

impl ProtocolVersion {
    /// The version two peers agree on, or `None` when they cannot talk.
    ///
    /// Per docs/reference/protocol.md: majors must be equal, and the agreed minor is the
    /// lower of the two, so neither side uses what the other lacks.
    #[must_use]
    pub fn negotiate(self, peer: Self) -> Option<Self> {
        (self.major == peer.major).then_some(Self {
            major: self.major,
            minor: self.minor.min(peer.minor),
        })
    }
}

impl From<ProtocolVersion> for crate::admin::ProtocolVersion {
    fn from(v: ProtocolVersion) -> Self {
        Self {
            major: v.major,
            minor: v.minor,
        }
    }
}

impl From<crate::admin::ProtocolVersion> for ProtocolVersion {
    fn from(v: crate::admin::ProtocolVersion) -> Self {
        Self {
            major: v.major,
            minor: v.minor,
        }
    }
}

/// The version of `protobuf/` this crate was generated from. Bump `minor` with
/// every compatible addition to the schema.
pub const PROTOCOL_VERSION: ProtocolVersion = ProtocolVersion { major: 1, minor: 6 };

/// The gRPC metadata key a client may set on any call, valued `major.minor`
/// (such as `1.0`). A daemon that cannot speak that major fails the call
/// with `FAILED_PRECONDITION` and `ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED`
/// rather than misreading it.
pub const VERSION_METADATA_KEY: &str = "norn-protocol-version";
