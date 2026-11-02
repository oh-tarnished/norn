//! The catalog's JSON form: one string a file's metadata can carry, readable by
//! any tool, and independent of protobuf field numbers.

use std::collections::BTreeMap;

use norn_protocol::common::ValueType;
use norn_protocol::entity::Entity;
use norn_protocol::signal::Signal;
use norn_protocol::stream::Stream;
use serde::{Deserialize, Serialize};

use crate::{AdapterError, Catalog, parse_value_type, value_type_name};

/// Bumped when a reader of an older form could misread a newer file.
const VERSION: u32 = 1;

#[derive(Serialize, Deserialize)]
struct Doc {
    version: u32,
    #[serde(default)]
    streams: Vec<Resource>,
    #[serde(default)]
    entities: Vec<Resource>,
    #[serde(default)]
    signals: Vec<SignalDoc>,
}

#[derive(Serialize, Deserialize)]
struct Resource {
    name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    display_name: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

#[derive(Serialize, Deserialize)]
struct SignalDoc {
    name: String,
    value_type: String,
    stream: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    display_name: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    unit: String,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    annotations: BTreeMap<String, String>,
}

fn sorted<K: Ord + Clone, V: Clone>(map: &std::collections::HashMap<K, V>) -> BTreeMap<K, V> {
    map.iter().map(|(k, v)| (k.clone(), v.clone())).collect()
}

impl Catalog {
    /// The JSON form, with annotations in key order, so equal catalogs are equal
    /// bytes.
    ///
    /// # Errors
    /// [`AdapterError::Encode`], which plain strings should never cause.
    pub fn to_json(&self) -> Result<String, AdapterError> {
        let resource = |name: &str, display: &str, a: &std::collections::HashMap<_, _>| Resource {
            name: name.to_owned(),
            display_name: display.to_owned(),
            annotations: sorted(a),
        };
        let doc = Doc {
            version: VERSION,
            streams: self
                .streams
                .iter()
                .map(|s| resource(&s.name, &s.display_name, &s.annotations))
                .collect(),
            entities: self
                .entities
                .iter()
                .map(|e| resource(&e.name, &e.display_name, &e.annotations))
                .collect(),
            signals: self
                .signals
                .iter()
                .map(|s| SignalDoc {
                    name: s.name.clone(),
                    value_type: value_type_name(
                        ValueType::try_from(s.value_type).unwrap_or(ValueType::Unspecified),
                    )
                    .to_owned(),
                    stream: s.stream.clone(),
                    display_name: s.display_name.clone(),
                    unit: s.unit.clone(),
                    annotations: sorted(&s.annotations),
                })
                .collect(),
        };
        serde_json::to_string(&doc).map_err(|e| AdapterError::Encode {
            format: "catalog",
            detail: e.to_string(),
        })
    }

    /// Reads [`to_json`](Self::to_json)'s form. Copies everything out of `text`.
    ///
    /// # Errors
    /// [`AdapterError::Malformed`] for bad JSON, a newer version, or a value type
    /// that does not exist.
    pub fn from_json(text: &str, format: &'static str) -> Result<Self, AdapterError> {
        let bad = |detail: String| AdapterError::Malformed {
            format,
            row: None,
            detail,
        };
        let doc: Doc = serde_json::from_str(text).map_err(|e| bad(format!("catalog: {e}")))?;
        if doc.version != VERSION {
            return Err(bad(format!("catalog version {} is newer", doc.version)));
        }
        let mut signals = Vec::with_capacity(doc.signals.len());
        for s in doc.signals {
            let Some(kind) = parse_value_type(&s.value_type) else {
                return Err(bad(format!("catalog: value type {:?}", s.value_type)));
            };
            signals.push(Signal {
                name: s.name,
                display_name: s.display_name,
                value_type: kind as i32,
                stream: s.stream,
                unit: s.unit,
                annotations: s.annotations.into_iter().collect(),
                ..Signal::default()
            });
        }
        Ok(Self {
            streams: (doc.streams.into_iter())
                .map(|r| Stream {
                    name: r.name,
                    display_name: r.display_name,
                    annotations: r.annotations.into_iter().collect(),
                    ..Stream::default()
                })
                .collect(),
            entities: (doc.entities.into_iter())
                .map(|r| Entity {
                    name: r.name,
                    display_name: r.display_name,
                    annotations: r.annotations.into_iter().collect(),
                    ..Entity::default()
                })
                .collect(),
            signals,
        })
    }
}
