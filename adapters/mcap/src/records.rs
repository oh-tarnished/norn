//! What a file's records mean: schemas and channels remembered, messages decoded.

use std::collections::HashMap;

use mcap::McapError;
use mcap::records::Record;
use norn_adapter::AdapterError;
use norn_protocol::common::Sample;
use prost::Message;

use crate::{CATALOG_FIELD, CATALOG_RECORD, FORMAT, PROTOBUF, SCHEMA_NAME};

/// What a meaningful record held.
pub(crate) enum Step {
    Catalog(String),
    Sample(Sample),
}

/// The mcap crate's error, by what it means for the file. `row` is the message
/// being read.
pub(crate) fn failed(e: McapError, row: u64) -> AdapterError {
    let (format, row, detail) = (FORMAT, Some(row), e.to_string());
    match e {
        McapError::UnexpectedEof | McapError::UnexpectedEoc => AdapterError::Truncated {
            format,
            row,
            detail,
        },
        McapError::BadMagic
        | McapError::BadChunkCrc { .. }
        | McapError::BadDataCrc { .. }
        | McapError::BadSummaryCrc { .. }
        | McapError::BadChunkLength { .. }
        | McapError::Parse(_) => AdapterError::Corrupt {
            format,
            row,
            detail,
        },
        _ => AdapterError::Malformed {
            format,
            row,
            detail,
        },
    }
}

/// The schemas and channels seen so far: a line per channel.
#[derive(Debug, Default)]
pub(crate) struct Directory {
    schemas: HashMap<u16, bool>,
    channels: HashMap<u16, String>,
    streams: HashMap<String, String>,
}

impl Directory {
    /// The stream a channel's metadata gives for `signal`; empty if none.
    pub(crate) fn stream_of(&self, signal: &str) -> &str {
        self.streams.get(signal).map_or("", String::as_str)
    }

    /// Takes in one record, read while on message `row`.
    pub(crate) fn absorb(
        &mut self,
        record: Record<'_>,
        row: u64,
    ) -> Result<Option<Step>, AdapterError> {
        let malformed = |detail: String| AdapterError::Malformed {
            format: FORMAT,
            row: Some(row),
            detail,
        };
        match record {
            Record::Schema { header, .. } => {
                let ours = header.name == SCHEMA_NAME && header.encoding == PROTOBUF;
                self.schemas.insert(header.id, ours);
            }
            Record::Channel(channel) => {
                let ours = self.schemas.get(&channel.schema_id).copied();
                if ours != Some(true) || channel.message_encoding != PROTOBUF {
                    let topic = channel.topic;
                    return Err(malformed(format!(
                        "channel {topic} is not {SCHEMA_NAME} over protobuf"
                    )));
                }
                let stream = channel.metadata.get("stream").cloned().unwrap_or_default();
                self.streams.insert(channel.topic.clone(), stream);
                self.channels.insert(channel.id, channel.topic);
            }
            Record::Metadata(metadata) if metadata.name == CATALOG_RECORD => {
                let json = metadata.metadata.get(CATALOG_FIELD).cloned();
                return Ok(json.map(Step::Catalog));
            }
            Record::Message { header, data } => {
                let Some(topic) = self.channels.get(&header.channel_id) else {
                    let id = header.channel_id;
                    return Err(malformed(format!("a message on unknown channel {id}")));
                };
                let decoded = Sample::decode(data.as_ref());
                let sample = decoded.map_err(|e| AdapterError::Corrupt {
                    format: FORMAT,
                    row: Some(row),
                    detail: e.to_string(),
                })?;
                if sample.signal != *topic {
                    let name = &sample.signal;
                    return Err(malformed(format!("{name} on channel {topic}")));
                }
                return Ok(Some(Step::Sample(sample)));
            }
            _ => {}
        }
        Ok(None)
    }
}
