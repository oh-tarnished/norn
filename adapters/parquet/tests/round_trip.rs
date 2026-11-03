//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Parquet and Arrow IPC on real files: exactness and refusing damage.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use std::fs::File;
use std::io::{BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

use norn_adapter::{AdapterError, SampleSink, SampleSource, time};
use norn_adapter_parquet::{ArrowSink, ArrowSource, ParquetSink, ParquetSource};
use norn_adapter_testkit::{catalog, file_round_trip, sample, signal_name, wire_bytes};
use norn_protocol::common::value::Kind;
use norn_protocol::common::{Quality, Sample, Value, ValueType};
use proptest::prelude::*;

static NEXT: AtomicUsize = AtomicUsize::new(0);

/// A path removed when dropped.
struct Temp(PathBuf);

impl Temp {
    fn new() -> Self {
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let name = format!("norn-columnar-{}-{n}", std::process::id());
        Self(std::env::temp_dir().join(name))
    }
    fn create(&self) -> BufWriter<File> {
        BufWriter::new(File::create(&self.0).unwrap())
    }
}

impl Drop for Temp {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn run_parquet(samples: &[Sample]) -> Vec<Sample> {
    let temp = Temp::new();
    let sink = ParquetSink::new(temp.create());
    let (declared, back) = file_round_trip(sink, samples, |_| {
        ParquetSource::new(File::open(&temp.0).unwrap())
    });
    assert_eq!(declared.to_json().unwrap(), catalog().to_json().unwrap());
    back
}

fn run_arrow(samples: &[Sample]) -> Vec<Sample> {
    let temp = Temp::new();
    let sink = ArrowSink::new(temp.create()).unwrap();
    let (declared, back) = file_round_trip(sink, samples, |_| {
        ArrowSource::new(BufReader::new(File::open(&temp.0).unwrap()))
    });
    assert_eq!(declared.to_json().unwrap(), catalog().to_json().unwrap());
    back
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]
    #[test]
    fn every_value_survives_parquet_exactly(
        samples in proptest::collection::vec(sample(-(1 << 62)..(1 << 62)), 0..40)
    ) {
        prop_assert_eq!(wire_bytes(&run_parquet(&samples)), wire_bytes(&samples));
    }

    #[test]
    fn every_value_survives_arrow_exactly(
        samples in proptest::collection::vec(sample(-(1 << 62)..(1 << 62)), 0..40)
    ) {
        prop_assert_eq!(wire_bytes(&run_arrow(&samples)), wire_bytes(&samples));
    }
}

fn samples(rows: i64) -> Vec<Sample> {
    (0..rows)
        .map(|i| Sample {
            signal: signal_name(ValueType::Double),
            observe_time: Some(time::from_nanos(i * 1_000)),
            value: Some(Value {
                kind: Some(Kind::DoubleValue((i as f64).sin())),
            }),
            quality: Some(Quality {
                severity: 1,
                substatus: 0,
            }),
            sequence: i,
        })
        .collect()
}

fn write_parquet(temp: &Temp, rows: i64) {
    let mut sink = ParquetSink::new(temp.create());
    sink.begin(&catalog()).unwrap();
    sink.write(&samples(rows)).unwrap();
    sink.finish().unwrap();
}

fn read_all(source: &mut dyn SampleSource) -> Result<u64, AdapterError> {
    source.catalog()?;
    let mut n = 0;
    while let Some(batch) = source.next_batch()? {
        n += batch.samples.len() as u64;
    }
    Ok(n)
}

#[test]
fn a_parquet_file_cut_short_says_so() {
    let temp = Temp::new();
    write_parquet(&temp, 1_000);
    let bytes = std::fs::read(&temp.0).unwrap();
    for keep in [0, 4, bytes.len() / 2, bytes.len() - 1] {
        std::fs::write(&temp.0, &bytes[..keep]).unwrap();
        let mut source = ParquetSource::new(File::open(&temp.0).unwrap());
        match read_all(&mut source) {
            Err(AdapterError::Truncated { .. }) => {}
            other => panic!("kept {keep}: {other:?}"),
        }
    }
}

#[test]
fn a_changed_byte_in_parquet_data_names_the_row_group_start() {
    let temp = Temp::new();
    // Two groups, so the damage is past the first row.
    write_parquet(&temp, 200_000);
    let clean = std::fs::read(&temp.0).unwrap();
    let mut hits = 0;
    for at in (clean.len() * 3 / 4..clean.len() * 3 / 4 + 400).step_by(37) {
        let mut bytes = clean.clone();
        bytes[at] ^= 0x10;
        std::fs::write(&temp.0, &bytes).unwrap();
        let mut source = ParquetSource::new(File::open(&temp.0).unwrap());
        match read_all(&mut source) {
            Err(AdapterError::Corrupt { row: Some(row), .. }) => {
                hits += 1;
                assert!(row > 1, "{row}");
            }
            other => panic!("flip at {at}: {other:?}"),
        }
    }
    assert!(hits > 0);
}

#[test]
fn a_cut_arrow_file_is_refused() {
    let temp = Temp::new();
    let mut sink = ArrowSink::new(temp.create()).unwrap();
    sink.begin(&catalog()).unwrap();
    sink.write(&samples(1_000)).unwrap();
    sink.finish().unwrap();
    drop(sink);
    let bytes = std::fs::read(&temp.0).unwrap();
    std::fs::write(&temp.0, &bytes[..bytes.len() / 2]).unwrap();
    let mut source = ArrowSource::new(File::open(&temp.0).unwrap());
    assert!(matches!(
        read_all(&mut source),
        Err(AdapterError::Truncated { .. })
    ));
}

#[test]
fn a_row_group_stays_bounded_however_much_is_written_at_once() {
    use parquet::file::reader::{FileReader, SerializedFileReader};
    let temp = Temp::new();
    write_parquet(&temp, 300_000);
    let reader = SerializedFileReader::new(File::open(&temp.0).unwrap()).unwrap();
    let groups = reader.metadata().row_groups();
    assert!(groups.len() >= 3, "{}", groups.len());
    assert!(groups.iter().all(|g| g.num_rows() <= 128 * 1024));
}
