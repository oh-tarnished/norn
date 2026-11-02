//! Writing MCAP.

use std::collections::{BTreeMap, HashMap};
use std::io::{Seek, SeekFrom, Write};

use mcap::records::{MessageHeader, Metadata};
use mcap::{McapError, WriteOptions, Writer};
use norn_adapter::{AdapterError, Catalog, SampleSink, time, value_type_name};
use norn_protocol::common::{Sample, ValueType};
use norn_protocol::query::SampleOrder;
use prost::Message;

use crate::{CATALOG_FIELD, CATALOG_RECORD, FORMAT, PROTOBUF, SAMPLE_DESCRIPTOR, SCHEMA_NAME};

/// A chunk's size: the most the writer buffers, and what a reader must hold.
const CHUNK_BYTES: u64 = 4 * 1024 * 1024;

fn failed(e: McapError) -> AdapterError {
    AdapterError::Encode {
        format: FORMAT,
        detail: e.to_string(),
    }
}

/// A [`SampleSink`] writing one MCAP file to `W`.
///
/// Holds one chunk and its index, and a line per channel. A file's chunk index
/// grows with the file, at about 100 bytes a chunk. Concurrency: one task at a
/// time.
pub struct McapSink<W: Write + Seek> {
    out: Option<W>,
    writer: Option<Writer<W>>,
    channels: HashMap<String, (u16, u32)>,
    scratch: Vec<u8>,
}

impl<W: Write + Seek> McapSink<W> {
    /// A sink writing to `out`. Writes nothing until `begin`.
    #[must_use]
    pub fn new(out: W) -> Self {
        Self {
            out: Some(out),
            writer: None,
            channels: HashMap::new(),
            scratch: Vec::new(),
        }
    }

    /// The writer back after `finish`, for a test or to sync it.
    pub fn into_inner(self) -> Option<W> {
        self.out
    }
}

impl<W: Write + Seek> SampleSink for McapSink<W> {
    fn order(&self) -> SampleOrder {
        SampleOrder::ByTime
    }

    fn begin(&mut self, catalog: &Catalog) -> Result<(), AdapterError> {
        let Some(out) = self.out.take() else {
            return Ok(());
        };
        let options = WriteOptions::new()
            .compression(None)
            .chunk_size(Some(CHUNK_BYTES))
            .library(concat!("norn-adapter-mcap/", env!("CARGO_PKG_VERSION")));
        let mut writer = options.create(out).map_err(failed)?;
        let schema = writer.add_schema(SCHEMA_NAME, PROTOBUF, SAMPLE_DESCRIPTOR);
        let schema = schema.map_err(failed)?;
        let record = Metadata {
            name: CATALOG_RECORD.to_owned(),
            metadata: BTreeMap::from([(CATALOG_FIELD.to_owned(), catalog.to_json()?)]),
        };
        writer.write_metadata(&record).map_err(failed)?;
        for signal in &catalog.signals {
            let kind = ValueType::try_from(signal.value_type).unwrap_or(ValueType::Unspecified);
            let mut about = BTreeMap::from([
                ("stream".to_owned(), signal.stream.clone()),
                ("value_type".to_owned(), value_type_name(kind).to_owned()),
            ]);
            if !signal.unit.is_empty() {
                about.insert("unit".to_owned(), signal.unit.clone());
            }
            let id = writer.add_channel(schema, &signal.name, PROTOBUF, &about);
            let id = id.map_err(failed)?;
            self.channels.insert(signal.name.clone(), (id, 0));
        }
        self.writer = Some(writer);
        Ok(())
    }

    fn write(&mut self, samples: &[Sample]) -> Result<(), AdapterError> {
        let Some(writer) = &mut self.writer else {
            return Ok(());
        };
        for sample in samples {
            let Some((channel_id, count)) = self.channels.get_mut(&sample.signal) else {
                return Err(AdapterError::Encode {
                    format: FORMAT,
                    detail: format!("{} is not in the catalog", sample.signal),
                });
            };
            let at = sample.observe_time.as_ref().and_then(time::to_nanos);
            let at = u64::try_from(at.unwrap_or(0)).unwrap_or(0);
            let header = MessageHeader {
                channel_id: *channel_id,
                sequence: *count,
                log_time: at,
                publish_time: at,
            };
            *count = count.wrapping_add(1);
            self.scratch.clear();
            sample.encode_raw(&mut self.scratch);
            (writer.write_to_known_channel(&header, &self.scratch)).map_err(failed)?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, AdapterError> {
        let Some(mut writer) = self.writer.take() else {
            return Ok(0);
        };
        writer.finish().map_err(failed)?;
        let mut out = writer.into_inner();
        let size = out.seek(SeekFrom::End(0));
        let size = size.map_err(AdapterError::io("sizing the MCAP file"))?;
        out.flush()
            .map_err(AdapterError::io("flushing the MCAP file"))?;
        self.out = Some(out);
        Ok(size)
    }
}
