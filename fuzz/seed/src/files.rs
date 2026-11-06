//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! What is read back from disk, written by the writers that write it.

use std::fs;
use std::io::Cursor;
use std::path::PathBuf;

use norn_adapter::{SampleSink, time};
use norn_adapter_csv::CsvSink;
use norn_adapter_mcap::McapSink;
use norn_adapter_parquet::{ArrowSink, ParquetSink};
use norn_adapter_testkit::{catalog, signal_name};
use norn_core::{
    Limits, MetadataLimits, Quality, Sample, Sequence, SignalId, StreamId, Timestamp, Value,
    ValueKind,
};
use norn_metadata::{EntitySpec, Registry, SignalSpec, StreamSpec, Write};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{self, ValueType};
use norn_runtime::ingest::fuzzing::encode_record;
use norn_storage::{
    Chunk, CleanMarker, CleanSegment, SegmentHeader, SegmentName, SegmentWriter, StorageOptions,
};

use crate::Corpus;

const STREAM: StreamId = StreamId::new(7);

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("norn-seed-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).expect("a scratch directory");
    dir
}

fn samples(signal: u64, value: impl Fn(i64) -> Value) -> Vec<Sample> {
    (0..64)
        .map(|i| {
            Sample::new(
                Timestamp::from_nanos(i * 1_000_000_000),
                SignalId::new(signal),
                value(i),
                Quality::GOOD,
                Sequence::new(i as u64),
            )
        })
        .collect()
}

/// A sealed and an unsealed segment holding chunks of `make`.
fn segments(c: &Corpus, target: &str, chunks: &[Chunk], selector: u8) {
    let dir = temp(target);
    let opts = StorageOptions::default();
    let mut w = SegmentWriter::create(&dir, SegmentHeader::new(STREAM, 1), opts).expect("a writer");
    for chunk in chunks {
        w.append(chunk).expect("a chunk");
    }
    w.flush().expect("a flush");
    let name = SegmentName::of(&SegmentHeader::new(STREAM, 1)).file_name();
    let open = fs::read(dir.join(&name)).expect("the open segment");
    c.selected(target, "unsealed", selector, &open);
    let reader = SegmentWriter::chunks(&w).to_vec();
    w.seal().expect("a seal");
    if target == "storage_segment" {
        for (i, meta) in reader.iter().enumerate() {
            let at = meta.offset as usize + norn_storage::format::FRAME_HEADER_LEN;
            let body = open
                .get(at..at + meta.body_len as usize)
                .unwrap_or_default();
            let count = (meta.count as u16).to_le_bytes();
            let mut seed = vec![meta.kind.tag(), count[0], count[1], 1, 0, 9, 0, 0];
            seed.extend_from_slice(body);
            c.put("storage_chunk", &format!("{i}"), &seed);
        }
    }
    c.selected(
        target,
        "sealed",
        selector,
        &fs::read(dir.join(&name)).expect("the segment"),
    );
    let _ = fs::remove_dir_all(&dir);
}

fn storage(c: &Corpus) {
    let of = |s: &[Sample]| Chunk::from_samples(s).expect("a chunk");
    let chunks = [
        of(&samples(1, |i| Value::F64(20.0 + i as f64 / 4.0))),
        of(&samples(2, |i| Value::I64(i * i))),
        of(&samples(3, |i| Value::Bool(i % 3 == 0))),
        of(&samples(4, |i| Value::String(format!("s-{}", i % 5)))),
    ];
    segments(c, "storage_segment", &chunks, 0);
    c.selected("storage_segment", "repaired", 0xf0, &[]);
    let batch = samples(1, |i| Value::F64(i as f64));
    let record = encode_record(&batch).expect("a record");
    c.put("journal_record", "batch", &record);
    let bytes: Vec<Sample> = vec![Sample::new(
        Timestamp::from_nanos(0),
        SignalId::new(0),
        Value::Bytes(record),
        Quality::GOOD,
        Sequence::new(0),
    )];
    segments(c, "journal_segment", &[of(&bytes)], 0);

    let segment = |seq, samples| CleanSegment {
        name: SegmentName { seq, generation: 0 },
        bytes: 4096,
        samples,
        time_range: Some((Timestamp::from_nanos(1), Timestamp::from_nanos(9))),
    };
    let dir = temp("clean");
    let marker = CleanMarker::new(STREAM, [segment(1, 10), segment(2, 20)]);
    marker.save(&dir).expect("a marker");
    let saved = fs::read(dir.join(norn_storage::CLEAN_FILE)).expect("the marker");
    c.selected("storage_clean", "two", 1, &saved);
    let _ = fs::remove_dir_all(&dir);
}

fn metadata(c: &Corpus) {
    let dir = temp("metadata");
    let (r, _) =
        Registry::open(&dir, Limits::default(), MetadataLimits::default()).expect("a registry");
    let t = Timestamp::from_nanos(1);
    r.create_stream("plant", StreamSpec::default(), t, Write::Commit)
        .expect("a stream");
    r.create_entity("dev-01", EntitySpec::default(), t, Write::Commit)
        .expect("an entity");
    let spec = SignalSpec::new(ValueKind::F64, "plant".into());
    r.create_signal("dev-01", "temp", spec, t, Write::Commit)
        .expect("a signal");
    drop(r);
    let log = fs::read(dir.join("metadata.log")).expect("the log");
    c.selected("metadata_log", "seeded", 0, &log);
    let _ = fs::remove_dir_all(&dir);
}

fn adapter<S: SampleSink>(mut sink: S) -> S {
    let rows = (0..32).map(|i| common::Sample {
        signal: signal_name(ValueType::Double),
        observe_time: Some(time::from_nanos(i)),
        value: Some(common::Value {
            kind: Some(Kind::DoubleValue(i as f64)),
        }),
        quality: Some(common::Quality {
            severity: 1,
            substatus: 0,
        }),
        sequence: 0,
    });
    sink.begin(&catalog()).expect("a catalog");
    sink.write(&rows.collect::<Vec<_>>()).expect("rows");
    sink.finish().expect("a file");
    sink
}

fn adapters(c: &Corpus) {
    let csv = adapter(CsvSink::new(Vec::new()));
    c.put("adapter_csv", "export", &csv.into_inner());
    c.put(
        "adapter_csv",
        "bare",
        b"time,signal,stream,type,value,severity,substatus,sequence\n\
          2026-11-02T08:00:00.000000000Z,entities/a/signals/t,streams/s,double,21.5,GOOD,,0\n",
    );
    let mcap = adapter(McapSink::new(Cursor::new(Vec::new())));
    let mcap = mcap.into_inner().expect("mcap").into_inner();
    c.put("adapter_mcap", "export", &mcap);
    let dir = temp("columnar");
    let path = dir.join("export");
    drop(adapter(
        ArrowSink::new(fs::File::create(&path).expect("a file")).expect("arrow"),
    ));
    c.put(
        "adapter_arrow",
        "export",
        &fs::read(&path).expect("the export"),
    );
    drop(adapter(ParquetSink::new(
        fs::File::create(&path).expect("a file"),
    )));
    c.put(
        "adapter_parquet",
        "export",
        &fs::read(&path).expect("the export"),
    );
    let _ = fs::remove_dir_all(&dir);
}

pub fn write(c: &Corpus) {
    storage(c);
    metadata(c);
    adapters(c);
}
