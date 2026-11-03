//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Why an export or an import failed.

use std::fmt;

/// A failed export or import. Match on the variant: nothing here asks a caller
/// to read a message. `row` counts samples in the file from 1, the header not
/// included; `None` means the file's structure, before any row can be named.
#[derive(Debug)]
#[non_exhaustive]
pub enum AdapterError {
    /// Reading or writing the file failed.
    Io {
        /// What was being done.
        context: &'static str,
        /// The operating system's reason.
        source: std::io::Error,
    },
    /// The input is readable but is not what the format says it should be.
    Malformed {
        /// The format, such as `csv`.
        format: &'static str,
        /// The row, when one can be named.
        row: Option<u64>,
        /// What is wrong.
        detail: String,
    },
    /// The input ends before it should: an interrupted copy or write.
    Truncated {
        /// The format.
        format: &'static str,
        /// The row being read when it ended, when one can be named.
        row: Option<u64>,
        /// What was missing.
        detail: String,
    },
    /// A checksum or structure check failed: the bytes changed.
    Corrupt {
        /// The format.
        format: &'static str,
        /// The first row of the damaged part, when one can be named.
        row: Option<u64>,
        /// What failed.
        detail: String,
    },
    /// A sample or catalog cannot be expressed in this format.
    Encode {
        /// The format.
        format: &'static str,
        /// Why.
        detail: String,
    },
    /// A bound was reached (SPEC §16). Raise the option, or split the work.
    LimitExceeded {
        /// The option or published limit.
        limit: &'static str,
        /// Its value.
        value: u64,
    },
    /// A call to the daemon failed.
    Call {
        /// The method, such as `QueryService.QuerySamples`.
        method: &'static str,
        /// What it returned, with its `ErrorInfo` detail inside.
        status: tonic::Status,
    },
    /// The daemon refused a batch and retrying cannot help, or a retry bound ran
    /// out. Nothing in that batch is stored.
    Refused {
        /// The `BackpressureReason` name, or the `ErrorReason` after admission.
        reason: String,
        /// What it said, with the first bad samples named.
        detail: String,
    },
    /// The file disagrees with what the daemon already holds, such as another
    /// value type for an existing signal, which a signal can never change.
    Conflict {
        /// The resource name.
        resource: String,
        /// How they differ.
        detail: String,
    },
    /// The daemon's response lacks something the protocol requires.
    Response {
        /// The method.
        method: &'static str,
        /// What is missing.
        detail: &'static str,
    },
}

fn at(f: &mut fmt::Formatter<'_>, format: &str, row: Option<u64>) -> fmt::Result {
    match row {
        Some(row) => write!(f, "{format} row {row}: "),
        None => write!(f, "{format} file structure: "),
    }
}

impl fmt::Display for AdapterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { context, source } => write!(f, "{context}: {source}"),
            Self::Malformed {
                format,
                row,
                detail,
            } => {
                at(f, format, *row)?;
                write!(f, "malformed: {detail}")
            }
            Self::Truncated {
                format,
                row,
                detail,
            } => {
                at(f, format, *row)?;
                write!(f, "truncated: {detail}")
            }
            Self::Corrupt {
                format,
                row,
                detail,
            } => {
                at(f, format, *row)?;
                write!(f, "corrupt: {detail}")
            }
            Self::Encode { format, detail } => write!(f, "{format}: cannot encode: {detail}"),
            Self::LimitExceeded { limit, value } => write!(f, "limit {limit} ({value}) exceeded"),
            Self::Call { method, status } => {
                write!(f, "{method}: {:?}: {}", status.code(), status.message())
            }
            Self::Refused { reason, detail } => write!(f, "refused, {reason}: {detail}"),
            Self::Conflict { resource, detail } => write!(f, "{resource}: conflict: {detail}"),
            Self::Response { method, detail } => write!(f, "{method}: response lacks {detail}"),
        }
    }
}

impl std::error::Error for AdapterError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Call { status, .. } => Some(status),
            _ => None,
        }
    }
}

impl AdapterError {
    /// An [`AdapterError::Io`] maker for `map_err`.
    pub fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> Self {
        move |source| Self::Io { context, source }
    }

    /// An [`AdapterError::Call`] maker for `map_err`.
    pub(crate) fn call(method: &'static str) -> impl FnOnce(tonic::Status) -> Self {
        move |status| Self::Call { method, status }
    }
}
