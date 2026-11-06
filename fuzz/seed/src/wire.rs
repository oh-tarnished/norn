//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What arrives over the wire: requests, ring frames and the text inside them.

use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, Sample, Value};
use norn_protocol::entity::{CreateEntityRequest, Entity};
use norn_protocol::telemetry::AppendSamplesRequest;
use prost::Message;

use crate::Corpus;

fn batch(n: i64) -> AppendSamplesRequest {
    let samples = (0..n).map(|i| Sample {
        signal: "entities/e/signals/double".to_owned(),
        observe_time: Some(norn_adapter::time::from_nanos(i)),
        value: Some(Value {
            kind: Some(Kind::DoubleValue(i as f64)),
        }),
        quality: Some(Quality {
            severity: 1,
            substatus: 0,
        }),
        sequence: i,
    });
    AppendSamplesRequest {
        stream: "streams/s".to_owned(),
        batch_id: 1,
        samples: samples.collect(),
        ack_level: 1,
    }
}

pub fn write(c: &Corpus) {
    for (name, n) in [("empty", 0), ("one", 1), ("many", 64)] {
        let bytes = batch(n).encode_to_vec();
        c.selected("server_append", name, 0, &bytes);
        c.put("shm_frame", name, &bytes);
    }
    let create = CreateEntityRequest {
        entity_id: "e".to_owned(),
        entity: Some(Entity {
            display_name: "an entity".to_owned(),
            ..Entity::default()
        }),
        ..CreateEntityRequest::default()
    };
    // Selector 2 is `CreateEntityRequest`, in the fuzz crate's list.
    c.selected("server_request", "create", 2, &create.encode_to_vec());

    // Empty elements cost the most decoding per byte: selectors 7 and 29 are the
    // batch creates. A message at the unary limit, 256 KiB.
    let empty = |field: u8| [field << 3 | 2, 0].repeat(128 << 10);
    c.selected("server_request", "entities", 7, &empty(1));
    c.selected("server_request", "signals", 29, &empty(2));

    let texts: [&[u8]; 5] = [
        b"quality.severity = QUALITY_SEVERITY_GOOD AND value > 20.5",
        b"value = 1 AND quality.substatus = QUALITY_SUBSTATUS_STALE",
        b"s.1700000000.4.entities/e/signals/t",
        b"w.17.entities/e/signals/t",
        b"",
    ];
    for (i, text) in texts.iter().enumerate() {
        c.put("query_text", &format!("{i}"), text);
    }
}
