//! What a format implements.

use norn_protocol::common::Sample;
use norn_protocol::query::SampleOrder;
use norn_protocol::signal::Signal;

use crate::{AdapterError, Catalog};

/// Where exported samples go: one file in one format.
///
/// The driver calls [`begin`](Self::begin) once, [`write`](Self::write) for each
/// bounded response of the query, and [`finish`](Self::finish) once. After an
/// error the sink is abandoned; the caller removes the partial output.
///
/// Implementations hold at most a bounded buffer: `write` flushes as it goes.
/// Concurrency: used by one task at a time.
pub trait SampleSink {
    /// The order the format wants samples in: by signal for columnar files, by
    /// time for a log a player reads forward.
    fn order(&self) -> SampleOrder;

    /// Declares everything the export will mention, before any sample.
    ///
    /// # Errors
    /// [`AdapterError`] when it cannot be written or does not fit the format.
    fn begin(&mut self, catalog: &Catalog) -> Result<(), AdapterError>;

    /// Writes samples, in [`order`](Self::order). Copies what it keeps.
    ///
    /// # Errors
    /// [`AdapterError`] when the output fails or a sample does not fit.
    fn write(&mut self, samples: &[Sample]) -> Result<(), AdapterError>;

    /// Completes the file, footer and all. Returns the bytes written.
    ///
    /// # Errors
    /// [`AdapterError`] when the output fails.
    fn finish(&mut self) -> Result<u64, AdapterError>;
}

/// One pull from a [`SampleSource`].
#[derive(Debug, Default, Clone, PartialEq)]
pub struct SourceBatch {
    /// Signals first seen in this batch, which the driver creates before
    /// appending: empty for a format that declares them all up front.
    pub signals: Vec<Signal>,
    /// The next samples, as many as the source holds at once.
    pub samples: Vec<Sample>,
}

/// Where imported samples come from: one file in one format.
///
/// The driver calls [`catalog`](Self::catalog) once, then
/// [`next_batch`](Self::next_batch) until `None`. A source reads forward and
/// holds at most a bounded batch; damage is an error naming the row, never a
/// short read (SPEC §20). Concurrency: used by one task at a time.
pub trait SampleSource {
    /// What the file declares up front; empty when it declares nothing.
    ///
    /// # Errors
    /// [`AdapterError`] when the file cannot be read, is damaged or is not this
    /// format.
    fn catalog(&mut self) -> Result<Catalog, AdapterError>;

    /// The next samples, or `None` at the verified end of the file.
    ///
    /// # Errors
    /// [`AdapterError`] naming the row at which the input stopped making sense.
    fn next_batch(&mut self) -> Result<Option<SourceBatch>, AdapterError>;
}
