//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A page of results: what a consumer gets at a time.

use std::sync::Arc;

use norn_core::Sample;

use crate::aggregate::Aggregate;
use crate::limits::QueryLimits;
use crate::memory::Charge;
use crate::token::Position;

/// One page's results, of one kind.
///
/// Concurrency: owned data.
#[derive(Debug)]
#[non_exhaustive]
pub enum Results {
    /// Samples, each with its signal's resource name, in the requested order.
    Samples(Vec<(Arc<str>, Sample)>),
    /// Aggregates, by signal then window.
    Aggregates(Vec<Aggregate>),
}

/// Results no bigger than `QueryLimits` allows, and where they got to.
///
/// Holding a page holds its charge, so an undelivered page still counts against
/// the budget. Drop it once delivered. Concurrency: `Send`.
#[derive(Debug)]
pub struct Page {
    /// The results, never empty.
    pub results: Results,
    /// A query resumed from this position continues just after this page.
    pub position: Position,
    /// What pays for `results`.
    pub charge: Charge,
}

/// A page being filled.
pub(crate) struct Filling<T> {
    pub(crate) items: Vec<T>,
    pub(crate) position: Option<Position>,
    pub(crate) bytes: u64,
}

impl<T> Filling<T> {
    pub(crate) fn new() -> Self {
        Self {
            items: Vec::new(),
            position: None,
            bytes: 0,
        }
    }

    pub(crate) fn full(&self, limits: &QueryLimits) -> bool {
        self.items.len() >= limits.page_results || self.bytes >= limits.page_bytes
    }

    pub(crate) fn add(&mut self, item: T, position: Position, bytes: u64) {
        self.items.push(item);
        self.position = Some(position);
        self.bytes += bytes;
    }
}
