//! Writing Parquet.

use std::collections::HashMap;
use std::io::Write;
use std::sync::Arc;

use norn_adapter::{AdapterError, CATALOG_KEY, Catalog, SampleSink};
use norn_protocol::common::Sample;
use norn_protocol::query::SampleOrder;
use parquet::arrow::ArrowWriter;
use parquet::basic::{Compression, Encoding};
use parquet::file::metadata::{KeyValue, SortingColumn};
use parquet::file::properties::WriterProperties;
use parquet::schema::types::ColumnPath;

use crate::DIGESTS_KEY;
use crate::columns::{schema, to_batch};
use crate::digest::{Digest, Part, encode};

const FORMAT: &str = "parquet";
/// Rows in a group; a group's columns are all that is buffered, so this and
/// [`GROUP_BYTES`] bound the writer.
const GROUP_ROWS: usize = 128 * 1024;
/// Encoded bytes in a group, for long strings.
const GROUP_BYTES: usize = 64 * 1024 * 1024;

fn failed(e: parquet::errors::ParquetError) -> AdapterError {
    AdapterError::Encode {
        format: FORMAT,
        detail: e.to_string(),
    }
}

/// A [`SampleSink`] writing one Parquet file to `W`, a group at a time.
///
/// Holds one row group and a digest per group. Concurrency: one task at a time.
pub struct ParquetSink<W: Write + Send> {
    out: Option<W>,
    writer: Option<ArrowWriter<W>>,
    streams: HashMap<String, String>,
    digest: Digest,
    parts: Vec<Part>,
}

impl<W: Write + Send> ParquetSink<W> {
    /// A sink writing to `out`. Writes nothing until `begin`.
    #[must_use]
    pub fn new(out: W) -> Self {
        Self {
            out: Some(out),
            writer: None,
            streams: HashMap::new(),
            digest: Digest::default(),
            parts: Vec::new(),
        }
    }

    /// Ends the row group, with its digest.
    fn seal(&mut self) -> Result<(), AdapterError> {
        if let Some(writer) = &mut self.writer {
            writer.flush().map_err(failed)?;
        }
        self.parts.push(self.digest.seal());
        Ok(())
    }

    fn properties(catalog: &Catalog) -> Result<WriterProperties, AdapterError> {
        let delta = |b: parquet::file::properties::WriterPropertiesBuilder, name: &str| {
            let path = ColumnPath::from(name);
            b.set_column_dictionary_enabled(path.clone(), false)
                .set_column_encoding(path, Encoding::DELTA_BINARY_PACKED)
        };
        let mut builder = WriterProperties::builder()
            .set_compression(Compression::SNAPPY)
            .set_max_row_group_row_count(None)
            .set_max_row_group_bytes(None)
            // signal, time, sequence.
            .set_sorting_columns(Some(
                [0, 3, 4]
                    .map(|column_idx| SortingColumn {
                        column_idx,
                        descending: false,
                        nulls_first: false,
                    })
                    .to_vec(),
            ))
            .set_key_value_metadata(Some(vec![KeyValue::new(
                CATALOG_KEY.to_owned(),
                catalog.to_json()?,
            )]));
        builder = delta(builder, "time");
        builder = delta(builder, "sequence");
        Ok(builder.build())
    }
}

impl<W: Write + Send> SampleSink for ParquetSink<W> {
    fn order(&self) -> SampleOrder {
        SampleOrder::BySignal
    }

    fn begin(&mut self, catalog: &Catalog) -> Result<(), AdapterError> {
        for signal in &catalog.signals {
            (self.streams).insert(signal.name.clone(), signal.stream.clone());
        }
        let Some(out) = self.out.take() else {
            return Ok(());
        };
        let properties = Self::properties(catalog)?;
        let writer = ArrowWriter::try_new(out, Arc::new(schema()), Some(properties));
        self.writer = Some(writer.map_err(failed)?);
        Ok(())
    }

    fn write(&mut self, samples: &[Sample]) -> Result<(), AdapterError> {
        // A response can outrun a group, so a group ends at a part boundary.
        for part in samples.chunks(GROUP_ROWS) {
            let batch = to_batch(part, &self.streams, FORMAT)?;
            self.digest.update(part);
            if let Some(writer) = &mut self.writer {
                writer.write(&batch).map_err(failed)?;
                let full = writer.in_progress_rows() >= GROUP_ROWS
                    || writer.in_progress_size() >= GROUP_BYTES;
                if full {
                    self.seal()?;
                }
            }
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, AdapterError> {
        self.seal()?;
        let Some(writer) = &mut self.writer else {
            return Ok(0);
        };
        let digests = encode(&self.parts);
        writer.append_key_value_metadata(KeyValue::new(DIGESTS_KEY.to_owned(), digests));
        writer.finish().map_err(failed)?;
        Ok(writer.bytes_written() as u64)
    }
}
