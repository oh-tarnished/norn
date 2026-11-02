//! Writing the format.

use std::collections::HashMap;
use std::io::Write;

use norn_adapter::{
    AdapterError, Catalog, SampleSink, severity_name, substatus_name, time, value_type_name,
    value_type_of,
};
use norn_protocol::common::{Sample, ValueType};
use norn_protocol::query::SampleOrder;

use crate::record::write_field;
use crate::value::write_value;
use crate::{FORMAT, HEADER, PREAMBLE, TRAILER};

/// A [`SampleSink`] over any writer; give it a `BufWriter`.
///
/// Holds one row's text, a line per signal, and whatever `W` buffers.
/// Concurrency: used by one task at a time. A failure leaves a partial file,
/// which an import reports as truncated.
#[derive(Debug)]
pub struct CsvSink<W> {
    out: W,
    streams: HashMap<String, String>,
    row: String,
    rows: u64,
    bytes: u64,
    digest: crc32fast::Hasher,
}

impl<W: Write> CsvSink<W> {
    /// A sink writing to `out`. Writes nothing until `begin`.
    #[must_use]
    pub fn new(out: W) -> Self {
        Self {
            out,
            streams: HashMap::new(),
            row: String::new(),
            rows: 0,
            bytes: 0,
            digest: crc32fast::Hasher::new(),
        }
    }

    /// The writer back, for a test or to sync it.
    pub fn into_inner(self) -> W {
        self.out
    }

    fn emit(&mut self, text: &[u8]) -> Result<(), AdapterError> {
        self.bytes += text.len() as u64;
        (self.out.write_all(text)).map_err(AdapterError::io("writing the CSV file"))
    }
}

fn missing(detail: &str) -> AdapterError {
    AdapterError::Encode {
        format: FORMAT,
        detail: detail.to_owned(),
    }
}

impl<W: Write> SampleSink for CsvSink<W> {
    fn order(&self) -> SampleOrder {
        SampleOrder::BySignal
    }

    fn begin(&mut self, catalog: &Catalog) -> Result<(), AdapterError> {
        for signal in &catalog.signals {
            (self.streams).insert(signal.name.clone(), signal.stream.clone());
        }
        let json = catalog.to_json()?;
        let head = format!("{PREAMBLE}{json}\n{HEADER}\n");
        self.emit(head.as_bytes())
    }

    fn write(&mut self, samples: &[Sample]) -> Result<(), AdapterError> {
        for sample in samples {
            let mut row = std::mem::take(&mut self.row);
            row.clear();
            let Some(stream) = self.streams.get(&sample.signal) else {
                let name = &sample.signal;
                return Err(missing(&format!("{name} is not in the catalog")));
            };
            let at = sample.observe_time.as_ref().and_then(time::to_nanos);
            let at = at.ok_or_else(|| missing("a sample has no time"))?;
            let kind = sample.value.as_ref().and_then(|v| v.kind.as_ref());
            let kind = kind.ok_or_else(|| missing("a sample has no value"))?;
            let quality = sample.quality.unwrap_or_default();
            row.push_str(&time::format_rfc3339(at));
            row.push(',');
            write_field(&sample.signal, &mut row);
            row.push(',');
            write_field(stream, &mut row);
            row.push(',');
            let kind_of: ValueType = value_type_of(kind);
            row.push_str(value_type_name(kind_of));
            row.push(',');
            let mut text = String::new();
            write_value(kind, &mut text);
            write_field(&text, &mut row);
            row.push(',');
            row.push_str(&severity_name(quality.severity));
            row.push(',');
            row.push_str(&substatus_name(quality.substatus));
            row.push(',');
            row.push_str(&sample.sequence.to_string());
            row.push('\n');
            self.digest.update(row.as_bytes());
            self.rows += 1;
            let done = self.emit(row.as_bytes());
            self.row = row;
            done?;
        }
        Ok(())
    }

    fn finish(&mut self) -> Result<u64, AdapterError> {
        let crc = self.digest.clone().finalize();
        let tail = format!("{TRAILER}rows={} crc32={crc:08x}\n", self.rows);
        self.emit(tail.as_bytes())?;
        self.out
            .flush()
            .map_err(AdapterError::io("flushing the CSV file"))?;
        Ok(self.bytes)
    }
}
