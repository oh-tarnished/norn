//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Why the daemon could not start or stopped abnormally.

use std::fmt;

use norn_database::DatabaseError;
use norn_ipc::IpcError;
use norn_metadata::MetadataError;
use norn_runtime::RuntimeError;
use norn_runtime::ingest::RecoverError;

/// Why the daemon could not start or stopped abnormally.
#[derive(Debug)]
#[non_exhaustive]
pub enum DaemonError {
    /// Binding or serving the endpoint failed.
    Ipc(IpcError),
    /// The server would not start or did not stop cleanly.
    Server(tpp_grpc::Error),
    /// The committed protocol descriptors could not form a reflection service.
    Reflection(tonic_reflection::server::Error),
    /// The runtime refused the configuration, or a task failed.
    Runtime(RuntimeError),
    /// The registry could not be opened.
    Metadata(MetadataError),
    /// The database is of a store version this build must not open.
    Database(DatabaseError),
    /// Recovery could not assess a stream.
    Storage(RecoverError),
    /// The recovery task panicked.
    RecoveryPanicked,
    /// Recovery finished without opening the temporal index: a bug.
    IndexMissing,
}

impl fmt::Display for DaemonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Ipc(e) => write!(f, "{e}"),
            Self::Server(e) => write!(f, "{e}"),
            Self::Reflection(e) => write!(f, "cannot build gRPC reflection: {e}"),
            Self::Runtime(e) => write!(f, "{e}"),
            Self::Metadata(e) => write!(f, "{e}"),
            Self::Database(e) => write!(f, "{e}"),
            Self::Storage(e) => write!(f, "{e}"),
            Self::RecoveryPanicked => f.write_str("recovery panicked"),
            Self::IndexMissing => f.write_str("recovery left no temporal index"),
        }
    }
}

impl std::error::Error for DaemonError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Ipc(e) => Some(e),
            Self::Server(e) => Some(e),
            Self::Reflection(e) => Some(e),
            Self::Runtime(e) => Some(e),
            Self::Metadata(e) => Some(e),
            Self::Database(e) => Some(e),
            Self::Storage(e) => Some(e),
            Self::RecoveryPanicked | Self::IndexMissing => None,
        }
    }
}

impl From<MetadataError> for DaemonError {
    fn from(e: MetadataError) -> Self {
        Self::Metadata(e)
    }
}

impl From<IpcError> for DaemonError {
    fn from(e: IpcError) -> Self {
        Self::Ipc(e)
    }
}

impl From<RuntimeError> for DaemonError {
    fn from(e: RuntimeError) -> Self {
        Self::Runtime(e)
    }
}
