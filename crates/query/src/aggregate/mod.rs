//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Reductions over fixed windows.

mod accum;
mod reduce;
mod windows;

pub(crate) use accum::Accum;
pub(crate) use reduce::Reduce;
pub(crate) use windows::Windows;

use std::sync::Arc;

use norn_core::{Timestamp, Value};

/// The result of a reduction for one signal and one window. A window with no
/// matching samples produces none.
///
/// Concurrency: owned data.
#[derive(Debug, Clone, PartialEq)]
#[non_exhaustive]
pub struct Aggregate {
    /// The signal's resource name.
    pub signal: Arc<str>,
    /// The window's first instant; `None` when it reaches back without limit.
    pub start: Option<Timestamp>,
    /// The first instant after the window; `None` when it has no limit.
    pub end: Option<Timestamp>,
    /// The result: [`Value::I64`] for COUNT, [`Value::F64`] for AVERAGE, SUM and
    /// RATE, a value of the signal's type otherwise.
    pub value: Value,
    /// How many samples it reduced.
    pub samples: u64,
}
