//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The MCAP format, on bytes in memory: exactness, the schema, and refusing damage.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::collections::BTreeSet;
use std::io::Cursor;

use mcap::read::{MessageStream, Summary};
use norn_adapter::{AdapterError, SampleSink, SampleSource, time};
use norn_adapter_mcap::{McapSink, McapSource, SAMPLE_DESCRIPTOR, SCHEMA_NAME};
use norn_adapter_testkit::{catalog, file_round_trip, sample, signal_name, wire_bytes};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, Sample, Value, ValueType};
use proptest::prelude::*;
use prost::Message;

fn run(samples: &[Sample]) -> Vec<Sample> {
    let (declared, back) =
        file_round_trip(McapSink::new(Cursor::new(Vec::new())), samples, |sink| {
            let mut bytes = sink.into_inner().unwrap();
            bytes.set_position(0);
            McapSource::new(bytes)
        });
    assert_eq!(declared.to_json().unwrap(), catalog().to_json().unwrap());
    back
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(64))]
    #[test]
    fn every_value_survives_export_and_import_exactly(
        samples in proptest::collection::vec(sample(-(1 << 62)..(1 << 62)), 0..40)
    ) {
        prop_assert_eq!(wire_bytes(&run(&samples)), wire_bytes(&samples));
    }
}

fn file(rows: i64) -> Vec<u8> {
    let samples: Vec<Sample> = (0..rows)
        .map(|i| Sample {
            signal: signal_name(ValueType::Double),
            observe_time: Some(time::from_nanos(i)),
            value: Some(Value {
                kind: Some(Kind::DoubleValue(i as f64)),
            }),
            quality: Some(Quality {
                severity: 1,
                substatus: 0,
            }),
            sequence: 0,
        })
        .collect();
    let mut sink = McapSink::new(Cursor::new(Vec::new()));
    sink.begin(&catalog()).unwrap();
    sink.write(&samples).unwrap();
    sink.finish().unwrap();
    sink.into_inner().unwrap().into_inner()
}

fn read(bytes: Vec<u8>) -> Result<u64, AdapterError> {
    let mut source = McapSource::new(Cursor::new(bytes));
    source.catalog()?;
    let mut n = 0;
    while let Some(batch) = source.next_batch()? {
        n += batch.samples.len() as u64;
    }
    Ok(n)
}

#[test]
fn the_committed_descriptor_declares_the_sample_and_all_it_imports() {
    let set = prost_types::FileDescriptorSet::decode(SAMPLE_DESCRIPTOR).unwrap();
    let names: BTreeSet<&str> = set.file.iter().map(|f| f.name()).collect();
    for file in &set.file {
        for import in &file.dependency {
            assert!(names.contains(import.as_str()), "{import} is missing");
        }
    }
    let file = set
        .file
        .iter()
        .find(|f| f.name() == "norn/type/sample.proto")
        .unwrap();
    assert_eq!(file.package(), "norn.type");
    assert_eq!(
        format!("{}.{}", file.package(), file.message_type[0].name()),
        SCHEMA_NAME
    );
}

#[test]
fn another_mcap_reader_sees_one_channel_per_signal_with_the_schema() {
    let bytes = file(3);
    let summary = Summary::read(&bytes).unwrap().unwrap();
    assert_eq!(summary.channels.len(), catalog().signals.len());
    assert_eq!(summary.stats.unwrap().message_count, 3);
    let first = MessageStream::new(&bytes).unwrap().next().unwrap().unwrap();
    assert_eq!(first.channel.topic, signal_name(ValueType::Double));
    let schema = first.channel.schema.clone().unwrap();
    assert_eq!(
        (schema.name.as_str(), schema.encoding.as_str()),
        (SCHEMA_NAME, "protobuf")
    );
    assert_eq!(first.channel.metadata["stream"], "streams/s");
    assert_eq!(first.channel.metadata["unit"], "Cel");
    assert!(Sample::decode(first.data.as_ref()).is_ok());
}

#[test]
fn a_file_cut_short_is_truncated_at_every_length_tried() {
    let bytes = file(200);
    for keep in (0..bytes.len() - 1).step_by(499) {
        let cut = bytes[..keep].to_vec();
        match read(cut) {
            Err(AdapterError::Truncated { .. }) => {}
            other => panic!("cut at {keep} of {}: {other:?}", bytes.len()),
        }
    }
}

#[test]
fn a_changed_byte_never_passes_for_data() {
    let bytes = file(200);
    let want = read(bytes.clone()).unwrap();
    for at in (8..bytes.len() - 8).step_by(211) {
        let mut hit = bytes.clone();
        hit[at] ^= 0x40;
        match read(hit) {
            Err(_) => {}
            Ok(n) => assert_eq!(n, want, "a flip at {at} changed the rows"),
        }
    }
}

#[test]
fn a_channel_of_another_schema_is_refused_by_name() {
    let mut out = Cursor::new(Vec::new());
    let mut writer = mcap::WriteOptions::new()
        .compression(None)
        .create(&mut out)
        .unwrap();
    let schema = writer.add_schema("other.Thing", "protobuf", b"").unwrap();
    let channel = writer
        .add_channel(schema, "/thing", "protobuf", &Default::default())
        .unwrap();
    let header = mcap::records::MessageHeader {
        channel_id: channel,
        sequence: 0,
        log_time: 0,
        publish_time: 0,
    };
    writer.write_to_known_channel(&header, b"").unwrap();
    writer.finish().unwrap();
    drop(writer);
    match read(out.into_inner()) {
        Err(AdapterError::Malformed { detail, .. }) => assert!(detail.contains("/thing")),
        other => panic!("{other:?}"),
    }
}
