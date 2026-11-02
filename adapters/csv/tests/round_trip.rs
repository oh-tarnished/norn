//! The CSV format, on bytes in memory: exactness and refusing damage.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::io::Cursor;

use norn_adapter::{AdapterError, SampleSink, SampleSource};
use norn_adapter_csv::{CsvSink, CsvSource};
use norn_adapter_testkit::{catalog, file_round_trip, sample, wire_bytes};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, Sample, Value};
use proptest::prelude::*;

fn run(samples: &[Sample]) -> Vec<Sample> {
    let (declared, back) = file_round_trip(CsvSink::new(Vec::new()), samples, |sink| {
        CsvSource::new(Cursor::new(sink.into_inner()))
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

fn file(rows: usize) -> Vec<u8> {
    let name = norn_adapter_testkit::signal_name(norn_protocol::common::ValueType::Double);
    let samples: Vec<Sample> = (0..rows as i64)
        .map(|i| Sample {
            signal: name.clone(),
            observe_time: Some(norn_adapter::time::from_nanos(i)),
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
    let mut sink = CsvSink::new(Vec::new());
    sink.begin(&catalog()).unwrap();
    sink.write(&samples).unwrap();
    sink.finish().unwrap();
    sink.into_inner()
}

fn read(bytes: Vec<u8>) -> Result<u64, AdapterError> {
    let mut source = CsvSource::new(Cursor::new(bytes));
    source.catalog()?;
    let mut n = 0;
    while let Some(batch) = source.next_batch()? {
        n += batch.samples.len() as u64;
    }
    Ok(n)
}

#[test]
fn a_file_cut_inside_a_row_names_that_row() {
    let mut bytes = file(5);
    let end = bytes.len() - b"# norn.end rows=5 crc32=00000000\n".len();
    bytes.truncate(end - 3);
    match read(bytes) {
        Err(AdapterError::Truncated { row: Some(5), .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_cut_exactly_between_rows_is_still_truncated() {
    let mut bytes = file(5);
    bytes.truncate(bytes.len() - b"# norn.end rows=5 crc32=00000000\n".len());
    match read(bytes) {
        Err(AdapterError::Truncated { row: Some(6), .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_changed_digit_fails_the_checksum() {
    let mut bytes = file(5);
    let text = String::from_utf8(bytes.clone()).unwrap();
    let at = text.rfind(",double,3,").unwrap() + ",double,".len();
    bytes[at] = b'7';
    assert!(matches!(read(bytes), Err(AdapterError::Corrupt { .. })));
}

#[test]
fn nonsense_in_a_row_names_the_row() {
    let text = String::from_utf8(file(5)).unwrap();
    let bad = text.replace(",double,2,", ",double,two,");
    match read(bad.into_bytes()) {
        Err(AdapterError::Malformed {
            row: Some(3),
            detail,
            ..
        }) => {
            assert!(detail.contains("two"), "{detail}");
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_file_by_hand_needs_only_the_header() {
    let text = "time,signal,stream,type,value,severity,substatus,sequence\n\
                2026-11-02T08:00:00Z,entities/a/signals/b,streams/s,int32,7,GOOD,,0";
    let mut source = CsvSource::new(Cursor::new(text.as_bytes()));
    assert_eq!(source.catalog().unwrap(), norn_adapter::Catalog::default());
    let batch = source.next_batch().unwrap().unwrap();
    assert_eq!(batch.signals.len(), 1);
    assert_eq!(batch.samples.len(), 1);
}

#[test]
fn a_row_contradicting_an_earlier_one_is_refused() {
    let text = "time,signal,stream,type,value,severity,substatus,sequence\n\
                2026-11-02T08:00:00Z,entities/a/signals/b,streams/s,int32,7,GOOD,,0\n\
                2026-11-02T08:00:01Z,entities/a/signals/b,streams/s,double,7,GOOD,,0\n";
    match read(text.as_bytes().to_vec()) {
        Err(AdapterError::Malformed { row: Some(2), .. }) => {}
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_record_past_the_bound_is_refused_rather_than_buffered() {
    let long = "x".repeat(1_000);
    let text = format!("{}\n{long}\n", norn_adapter_csv::HEADER);
    let mut source = CsvSource::new(Cursor::new(text.into_bytes())).max_record_bytes(100);
    source.catalog().unwrap();
    assert!(matches!(
        source.next_batch(),
        Err(AdapterError::LimitExceeded { .. })
    ));
}

#[test]
fn a_batch_never_outgrows_its_bound_however_long_the_file() {
    let mut source = CsvSource::new(Cursor::new(file(20_000)));
    source.catalog().unwrap();
    let mut most = 0;
    while let Some(batch) = source.next_batch().unwrap() {
        most = most.max(batch.samples.len());
    }
    assert_eq!(most, 4_096);
}
