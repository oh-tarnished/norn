//! Reading MCAP forward, verifying CRCs as it goes.

use std::io::Read;

use mcap::parse_record;
use mcap::sans_io::linear_reader::{LinearReadEvent, LinearReader, LinearReaderOptions};
use norn_adapter::{
    AdapterError, Catalog, DEFAULT_MAX_SIGNALS, Discovery, SampleSource, SourceBatch, value_type_of,
};
use norn_protocol::common::Sample;
use prost::Message;

use crate::FORMAT;
use crate::records::{Directory, Step, failed};

/// Messages a batch holds.
const BATCH_ROWS: usize = 4_096;
/// Bytes a batch holds, whichever comes first.
const BATCH_BYTES: usize = 4 * 1024 * 1024;
/// The longest record, a whole chunk included: a bound on a damaged length too.
const MAX_RECORD_BYTES: usize = 256 * 1024 * 1024;
/// How much to read at once.
const READ_BYTES: usize = 256 * 1024;

/// A [`SampleSource`] reading one MCAP file.
///
/// Holds one batch, one chunk, and a line per channel. Rows count messages from 1.
/// Every channel must be `norn.type.Sample` protobuf; any other is refused rather
/// than skipped. Concurrency: one task at a time.
pub struct McapSource<R> {
    input: R,
    reader: LinearReader,
    directory: Directory,
    pending: Option<Sample>,
    rows: u64,
    discovery: Discovery,
}

impl<R: Read> McapSource<R> {
    /// A source reading `input`. Reads nothing until [`SampleSource::catalog`].
    #[must_use]
    pub fn new(input: R) -> Self {
        let options = LinearReaderOptions::default()
            .with_prevalidate_chunk_crcs(true)
            .with_validate_data_section_crc(true)
            .with_validate_summary_section_crc(true)
            .with_record_length_limit(MAX_RECORD_BYTES);
        Self {
            input,
            reader: LinearReader::new_with_options(options),
            directory: Directory::default(),
            pending: None,
            rows: 0,
            discovery: Discovery::new(FORMAT, DEFAULT_MAX_SIGNALS),
        }
    }

    /// The next catalog or sample; `None` at the verified end.
    fn step(&mut self) -> Result<Option<Step>, AdapterError> {
        let row = self.rows + 1;
        loop {
            let Some(event) = self.reader.next_event() else {
                return Ok(None);
            };
            match event.map_err(|e| failed(e, row))? {
                LinearReadEvent::ReadRequest(hint) => {
                    let room = self.reader.insert(hint.clamp(1, READ_BYTES));
                    let n = self.input.read(room);
                    let n = n.map_err(AdapterError::io("reading the MCAP file"))?;
                    self.reader.notify_read(n);
                }
                LinearReadEvent::Record { opcode, data } => {
                    let record = parse_record(opcode, data).map_err(|e| failed(e, row))?;
                    if let Some(step) = self.directory.absorb(record, row)? {
                        return Ok(Some(step));
                    }
                }
            }
        }
    }

    fn observe(&mut self, sample: &Sample, out: &mut SourceBatch) -> Result<(), AdapterError> {
        let row = self.rows + 1;
        let kind = sample.value.as_ref().and_then(|v| v.kind.as_ref());
        let kind = kind.ok_or_else(|| AdapterError::Malformed {
            format: FORMAT,
            row: Some(row),
            detail: "a sample with no value".to_owned(),
        })?;
        let stream = self.directory.stream_of(&sample.signal);
        let kind = value_type_of(kind);
        out.signals
            .extend(self.discovery.observe(&sample.signal, kind, stream, row)?);
        self.rows = row;
        Ok(())
    }
}

impl<R: Read> SampleSource for McapSource<R> {
    fn catalog(&mut self) -> Result<Catalog, AdapterError> {
        match self.step()? {
            Some(Step::Catalog(json)) => {
                let catalog = Catalog::from_json(&json, FORMAT)?;
                catalog
                    .signals
                    .iter()
                    .for_each(|s| self.discovery.declare(s));
                Ok(catalog)
            }
            Some(Step::Sample(sample)) => {
                self.pending = Some(sample);
                Ok(Catalog::default())
            }
            None => Ok(Catalog::default()),
        }
    }

    fn next_batch(&mut self) -> Result<Option<SourceBatch>, AdapterError> {
        let mut batch = SourceBatch::default();
        let mut bytes = 0;
        while batch.samples.len() < BATCH_ROWS && bytes < BATCH_BYTES {
            let sample = match self.pending.take() {
                Some(sample) => sample,
                None => match self.step()? {
                    Some(Step::Sample(sample)) => sample,
                    Some(Step::Catalog(_)) => continue,
                    None => break,
                },
            };
            self.observe(&sample, &mut batch)?;
            bytes += sample.encoded_len();
            batch.samples.push(sample);
        }
        let empty = batch.samples.is_empty();
        Ok((!empty).then_some(batch))
    }
}
