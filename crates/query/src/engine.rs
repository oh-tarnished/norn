//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The engine, and one query pulled page by page.

use std::sync::Arc;

use norn_core::{Timestamp, ValueKind};
use norn_index::TemporalIndex;
use norn_storage::StorageOptions;

use crate::aggregate::{Reduce, Windows};
use crate::error::{QueryError, Result};
use crate::files::FileCache;
use crate::filter::Filter;
use crate::limits::QueryLimits;
use crate::memory::{Charge, QueryMemory};
use crate::newest::Newest;
use crate::query_run::{Query, Shape};
use crate::read::Reader;
use crate::resume::{Start, start_from};
use crate::spec::{Function, Order, Output, QuerySpec, Reduction, Target};
use crate::tail::TailSource;
use crate::walk::Walk;

/// What every query reads through.
///
/// Lifecycle: built once, by whatever assembles the daemon; it owns no thread and
/// needs no closing. Concurrency: `Send + Sync`, shared in an `Arc`; queries share
/// only the index and the budget. Memory: nothing of its own.
#[derive(Debug)]
pub struct Engine {
    pub(crate) index: Arc<TemporalIndex>,
    pub(crate) tail: Arc<dyn TailSource>,
    pub(crate) memory: Arc<dyn QueryMemory>,
    pub(crate) limits: QueryLimits,
    storage: StorageOptions,
}

/// What a query did. For tests, benchmarks and logs.
///
/// Concurrency: `Copy` plain data.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[non_exhaustive]
pub struct QueryStats {
    /// Chunks read and decoded. A chunk two slices share counts twice.
    pub chunks_read: u64,
    /// Bytes read from segment files, headers included.
    pub bytes_read: u64,
    /// Reads restarted because a file vanished.
    pub races: u64,
}

impl Engine {
    /// An engine over `index`, merging `tail`, charging `memory`.
    #[must_use]
    pub fn new(
        index: Arc<TemporalIndex>,
        tail: Arc<dyn TailSource>,
        memory: Arc<dyn QueryMemory>,
        limits: QueryLimits,
        storage: StorageOptions,
    ) -> Self {
        Self {
            index,
            tail,
            memory,
            limits,
            storage,
        }
    }

    /// A lookup of single signals' newest samples, sharing open files between them.
    #[must_use]
    pub fn newest(self: &Arc<Self>) -> Newest {
        let charge = Charge::new(Arc::clone(&self.memory));
        let files = FileCache::new(self.limits.open_files, self.storage, charge);
        Newest::new(self, files)
    }

    /// An empty charge against this engine's memory, for a layer above that holds
    /// results of its own.
    pub fn charge(&self) -> Charge {
        Charge::new(Arc::clone(&self.memory))
    }

    /// Checks `spec` and prepares it. Reads nothing from disk, so a bad request
    /// fails before any work, and the rest of a query's errors arrive by page.
    ///
    /// # Errors
    /// [`QueryError::NotApplicable`], [`QueryError::BadPosition`],
    /// [`QueryError::Memory`] when even the signal list does not fit.
    pub fn open(self: &Arc<Self>, spec: QuerySpec) -> Result<Query> {
        let QuerySpec {
            mut targets,
            range,
            filter,
            output,
            resume,
        } = spec;
        check(&targets, &filter, output)?;
        targets.sort_by(|a, b| a.name.cmp(&b.name));
        let mut base = Charge::new(Arc::clone(&self.memory));
        let names: u64 = targets.iter().map(|t| t.name.len() as u64 + 96).sum();
        base.grow(names)?;
        let list: Vec<(u32, Arc<Target>)> = (0u32..)
            .zip(targets)
            .map(|(i, t)| (i, Arc::new(t)))
            .collect();
        let reader = Reader {
            engine: Arc::clone(self),
            files: FileCache::new(
                self.limits.open_files,
                self.storage,
                Charge::new(Arc::clone(&self.memory)),
            ),
            chunks_read: 0,
            races: 0,
        };
        let bounds = range.inclusive();
        let windows = match output {
            Output::Aggregates(r) => Some(Windows::new(range, r.window)),
            Output::Samples(_) => None,
        };
        let (from, to) = bounds.unwrap_or((Timestamp::MAX, Timestamp::MIN));
        let start = match (bounds, &resume) {
            (None, _) => Start {
                skip: list.len(),
                ..Start::default()
            },
            (_, None) => Start::default(),
            (Some(_), Some(p)) => start_from(p, &list, output, windows.as_ref())?,
        };
        let shape = match output {
            Output::Aggregates(Reduction {
                function: Function::Latest,
                ..
            }) => Shape::Latest { next: start.skip },
            Output::Aggregates(r) => {
                let windows = windows.unwrap_or(Windows::new(range, r.window));
                Shape::Windows(Box::new(Reduce::new(r.function, windows)))
            }
            Output::Samples(_) => Shape::Raw,
        };
        let merged = matches!(output, Output::Samples(Order::ByTime));
        let walk = Walk::new(reader, list, merged, (from, to), filter, start);
        Ok(Query::new(walk, shape, (from, to), base))
    }
}

fn check(targets: &[Target], filter: &Filter, output: Output) -> Result<()> {
    let numeric = |k: ValueKind| {
        matches!(
            k,
            ValueKind::I32
                | ValueKind::I64
                | ValueKind::U32
                | ValueKind::U64
                | ValueKind::F32
                | ValueKind::F64
        )
    };
    for t in targets {
        if !filter.applies_to(t.kind) {
            let what = "sample_filter compares value with something of another type";
            return Err(not_applicable(t, what));
        }
        if let Output::Aggregates(r) = output
            && r.function.numeric()
            && !numeric(t.kind)
        {
            return Err(not_applicable(
                t,
                "AVERAGE, SUM and RATE need a numeric signal",
            ));
        }
    }
    Ok(())
}

fn not_applicable(t: &Target, what: &'static str) -> QueryError {
    QueryError::NotApplicable {
        signal: Arc::clone(&t.name),
        what,
    }
}
