//! SPEC §36 for interchange: export, then import into a fresh daemon, gives the
//! same samples, per format, through two real daemons.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod support;

use std::collections::BTreeMap;
use std::fs::File;
use std::io::{BufReader, BufWriter};

use norn_adapter::{
    AdapterError, ExportOptions, ImportOptions, SampleSink, SampleSource, Selection, export,
    import, time,
};
use norn_adapter_csv::{CsvSink, CsvSource};
use norn_adapter_mcap::{McapSink, McapSource};
use norn_adapter_parquet::{ArrowSink, ArrowSource, ParquetSink, ParquetSource};
use norn_adapter_testkit::{MemorySource, catalog, sample};
use norn_protocol::common::Sample;
use norn_protocol::query::QuerySamplesRequest;
use norn_protocol::query::query_service_client::QueryServiceClient;
use norn_protocol::telemetry::AcknowledgementLevel;
use proptest::prelude::*;
use prost::Message;
use support::Running;
use tonic::transport::Channel;

type Key = (String, i64, i64);

/// Everything the daemon holds, keyed, as wire bytes so NaN payloads compare.
async fn holdings(channel: Channel) -> BTreeMap<Key, Vec<u8>> {
    let request = QuerySamplesRequest {
        parent: "entities/-".to_owned(),
        ..QuerySamplesRequest::default()
    };
    let mut client = QueryServiceClient::new(channel);
    let mut replies = client.query_samples(request).await.unwrap().into_inner();
    let mut all = BTreeMap::new();
    while let Some(page) = replies.message().await.unwrap() {
        for s in page.samples {
            let at = time::to_nanos(s.observe_time.as_ref().unwrap()).unwrap();
            all.insert((s.signal.clone(), at, s.sequence), s.encode_to_vec());
        }
    }
    all
}

fn persisted() -> ImportOptions {
    ImportOptions::default().ack(AcknowledgementLevel::Persisted)
}

fn make_sink(format: &str, file: File) -> Box<dyn SampleSink> {
    let out = BufWriter::new(file);
    match format {
        "csv" => Box::new(CsvSink::new(out)),
        "parquet" => Box::new(ParquetSink::new(out)),
        "arrow" => Box::new(ArrowSink::new(out).unwrap()),
        _ => Box::new(McapSink::new(out)),
    }
}

fn make_source(format: &str, file: File) -> Box<dyn SampleSource> {
    match format {
        "csv" => Box::new(CsvSource::new(BufReader::new(file))),
        "parquet" => Box::new(ParquetSource::new(file)),
        "arrow" => Box::new(ArrowSource::new(BufReader::new(file))),
        _ => Box::new(McapSource::new(BufReader::new(file))),
    }
}

async fn round_trip(format: &str, samples: Vec<Sample>) -> Result<(), AdapterError> {
    let first = Running::start().await;
    let channel = first.channel().await;
    let count = samples.len();
    let mut memory = MemorySource {
        catalog: catalog(),
        samples: Some(samples),
    };
    import(channel.clone(), &mut memory, &persisted()).await?;
    let want = holdings(channel.clone()).await;
    assert_eq!(
        want.len(),
        count,
        "the daemon must hold every sample to compare"
    );

    let path = first.dir.0.join(format!("export.{format}"));
    let mut sink = make_sink(format, File::create(&path).unwrap());
    let chosen = Selection::new("entities/-");
    export(channel, &chosen, sink.as_mut(), &ExportOptions::default()).await?;
    drop(sink);

    let second = Running::start().await;
    let channel = second.channel().await;
    let mut source = make_source(format, File::open(&path).unwrap());
    import(channel.clone(), source.as_mut(), &persisted()).await?;
    let got = holdings(channel).await;
    assert_eq!(got, want, "{format}");
    second.stop().await;
    first.stop().await;
    Ok(())
}

/// Distinct times, so no two samples are one observation.
fn distinct(samples: Vec<Sample>) -> Vec<Sample> {
    let spread = samples.into_iter().enumerate();
    spread
        .map(|(i, mut s)| {
            let at = time::to_nanos(s.observe_time.as_ref().unwrap()).unwrap();
            s.observe_time = Some(time::from_nanos(at + i as i64));
            s
        })
        .collect()
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(6))]

    #[test]
    fn export_then_import_into_a_fresh_daemon_gives_identical_samples(
        samples in proptest::collection::vec(sample(-(1 << 61)..(1 << 61)), 30..60)
    ) {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2).enable_all().build().unwrap();
        let samples = distinct(samples);
        for format in ["csv", "parquet", "arrow", "mcap"] {
            runtime.block_on(round_trip(format, samples.clone())).unwrap();
        }
    }
}
