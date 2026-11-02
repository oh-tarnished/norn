//! Test support for Norn adapters; a dev-dependency only, never shipped.
//!
//! # Layer contract
//!
//! - **Owns**: [`proptest`] strategies producing samples of every value kind with
//!   the values that break careless codecs (NaN payloads, signed zeros, integer
//!   extremes, every quality), and a [`file_round_trip`] over any sink and source.
//! - **Must not**: touch a daemon, a file system or a clock.
//! - **May import**: `norn-adapter`, `norn-protocol`, `proptest`.
//! - **Spans**: none.

#![allow(
    clippy::expect_used,
    reason = "test support: a failure should stop the test"
)]

use norn_adapter::{AdapterError, Catalog, SampleSink, SampleSource, SourceBatch};
use norn_protocol::common::{Sample, ValueType};
use norn_protocol::entity::Entity;
use norn_protocol::signal::Signal;
use norn_protocol::stream::Stream;

mod strategies;

pub use strategies::{AWKWARD_DOUBLES, AWKWARD_FLOATS, quality, sample, value};

/// A signal of each value type, `entities/e/signals/...`, in one stream, and the
/// catalog declaring them, with metadata on every level.
#[must_use]
pub fn catalog() -> Catalog {
    let annotations = |k: &str, v: &str| [(k.to_owned(), v.to_owned())].into_iter().collect();
    let signals = (1..=10)
        .map(|n| {
            let kind = ValueType::try_from(n).unwrap_or(ValueType::Unspecified);
            Signal {
                name: signal_name(kind),
                display_name: format!("signal {n}"),
                value_type: n,
                stream: "streams/s".to_owned(),
                unit: "Cel".to_owned(),
                annotations: annotations("protocol", "BACnet"),
                ..Signal::default()
            }
        })
        .collect();
    Catalog {
        streams: vec![Stream {
            name: "streams/s".to_owned(),
            display_name: "a stream".to_owned(),
            annotations: annotations("site", "north"),
            ..Stream::default()
        }],
        entities: vec![Entity {
            name: "entities/e".to_owned(),
            display_name: "an entity".to_owned(),
            annotations: annotations("kind", "ahu"),
            ..Entity::default()
        }],
        signals,
    }
}

/// The name of the [`catalog`] signal of `kind`.
#[must_use]
pub fn signal_name(kind: ValueType) -> String {
    format!("entities/e/signals/{}", norn_adapter::value_type_name(kind))
}

/// A sample's bytes on the wire: equal bytes means equal sample, NaN payloads
/// included, where `==` on floats would say otherwise.
#[must_use]
pub fn wire_bytes(samples: &[Sample]) -> Vec<Vec<u8>> {
    samples.iter().map(prost::Message::encode_to_vec).collect()
}

/// A [`SampleSource`] over samples in memory, to fill a daemon in a test.
#[derive(Debug, Default)]
pub struct MemorySource {
    /// What the source declares.
    pub catalog: Catalog,
    /// The one batch it holds.
    pub samples: Option<Vec<Sample>>,
}

impl SampleSource for MemorySource {
    fn catalog(&mut self) -> Result<Catalog, AdapterError> {
        Ok(self.catalog.clone())
    }

    fn next_batch(&mut self) -> Result<Option<SourceBatch>, AdapterError> {
        let samples = self.samples.take();
        Ok(samples.map(|samples| SourceBatch {
            signals: Vec::new(),
            samples,
        }))
    }
}

/// Writes `samples` through `sink`, reads them through the source `reopen` builds,
/// and returns what came back, batches joined.
pub fn file_round_trip<K: SampleSink, O: SampleSource>(
    mut sink: K,
    samples: &[Sample],
    reopen: impl FnOnce(K) -> O,
) -> (Catalog, Vec<Sample>) {
    sink.begin(&catalog()).expect("begin");
    for part in samples.chunks(7) {
        sink.write(part).expect("write");
    }
    sink.finish().expect("finish");
    let mut source = reopen(sink);
    let declared = source.catalog().expect("catalog");
    let mut back = Vec::new();
    while let Some(batch) = source.next_batch().expect("next_batch") {
        back.extend(batch.samples);
    }
    (declared, back)
}
