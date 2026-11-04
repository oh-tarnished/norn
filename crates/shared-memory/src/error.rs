//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What can go wrong with a ring.

use std::fmt;

/// A failure of the data plane. Each variant is a reason to fall back to gRPC or
/// to end a call, and none loses a frame silently. Match on the variant.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum PlaneError {
    /// iceoryx2 refused `step`; `detail` is its error, for the log.
    Setup {
        /// What was being done.
        step: &'static str,
        /// iceoryx2's error, formatted.
        detail: String,
    },
    /// The peer's frame claims more bytes than a ring frame may hold. Nothing
    /// was read from it.
    FrameTooLarge {
        /// What the frame claims.
        length: usize,
        /// The ring's limit.
        limit: usize,
    },
    /// Receiving failed; the ring cannot be trusted further.
    Receive(String),
}

impl fmt::Display for PlaneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Setup { step, detail } => write!(f, "cannot {step}: {detail}"),
            Self::FrameTooLarge { length, limit } => {
                write!(f, "a {length}-byte frame exceeds the ring's {limit}")
            }
            Self::Receive(why) => write!(f, "cannot receive from the ring: {why}"),
        }
    }
}

impl std::error::Error for PlaneError {}

impl From<PlaneError> for std::io::Error {
    fn from(e: PlaneError) -> Self {
        Self::other(e)
    }
}
