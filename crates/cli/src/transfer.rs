//! `export` and `import`: a file format, driven through the public API.

use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Write};
use std::path::Path;

use norn_adapter::{
    ExportOptions, ImportOptions, SampleSink, SampleSource, Selection, export as run_export,
    import as run_import,
};
use norn_adapter_csv::{CsvSink, CsvSource};
use norn_adapter_mcap::{McapSink, McapSource};
use norn_adapter_parquet::{ArrowSink, ArrowSource, ParquetSink, ParquetSource};
use norn_protocol::google::r#type::Interval;
use norn_protocol::telemetry::AcknowledgementLevel;
use tonic::transport::Channel;

use crate::CliError;
use crate::query::{selection, timestamp};
use crate::resources::usage;

/// A file format.
#[derive(Debug, Clone, Copy)]
enum Format {
    Csv,
    Parquet,
    Arrow,
    Mcap,
}

fn format(word: &str) -> Result<Format, CliError> {
    match word {
        "csv" => Ok(Format::Csv),
        "parquet" => Ok(Format::Parquet),
        "arrow" => Ok(Format::Arrow),
        "mcap" => Ok(Format::Mcap),
        _ => Err(usage("--format is csv, parquet, arrow or mcap")),
    }
}

fn io(context: &'static str) -> impl FnOnce(std::io::Error) -> CliError {
    move |e| CliError::Adapter(norn_adapter::AdapterError::io(context)(e))
}

fn sink(format: Format, file: File) -> Result<Box<dyn SampleSink>, CliError> {
    let out = BufWriter::new(file);
    Ok(match format {
        Format::Csv => Box::new(CsvSink::new(out)),
        Format::Parquet => Box::new(ParquetSink::new(out)),
        Format::Arrow => Box::new(ArrowSink::new(out).map_err(CliError::Adapter)?),
        Format::Mcap => Box::new(McapSink::new(out)),
    })
}

fn source(format: Format, file: File) -> Box<dyn SampleSource> {
    match format {
        Format::Csv => Box::new(CsvSource::new(BufReader::new(file))),
        Format::Parquet => Box::new(ParquetSource::new(file)),
        Format::Arrow => Box::new(ArrowSource::new(BufReader::new(file))),
        Format::Mcap => Box::new(McapSource::new(BufReader::new(file))),
    }
}

/// `export TARGET --format F --output PATH [--from T] [--to T] [--select F]`. The
/// file must not exist; on failure the partial file is removed.
pub(crate) async fn export(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    const USE: &str = "export TARGET --format F --output PATH [--from T] [--to T] [--select F]";
    let Some((target, flags)) = words.split_first() else {
        return Err(usage(USE));
    };
    let (parent, mut filter) = selection(target);
    let (mut kind, mut path) = (None, None);
    let mut interval = Interval::default();
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = flags.next().ok_or_else(|| usage(USE))?;
        match *flag {
            "--format" => kind = Some(format(word)?),
            "--output" => path = Some(*word),
            "--from" => interval.start_time = Some(timestamp(word)?),
            "--to" => interval.end_time = Some(timestamp(word)?),
            "--select" if filter.is_empty() => filter = (*word).to_owned(),
            "--select" => filter = format!("{filter} AND {word}"),
            _ => return Err(usage(USE)),
        }
    }
    let (kind, path) = kind.zip(path).ok_or_else(|| usage(USE))?;
    let mut chosen = Selection::new(parent).filter(filter);
    if interval.start_time.is_some() || interval.end_time.is_some() {
        chosen = chosen.interval(interval);
    }
    let file = OpenOptions::new().write(true).create_new(true).open(path);
    let file = file.map_err(io("creating the output, which must not exist"))?;
    let mut file_sink = sink(kind, file)?;
    let options = ExportOptions::default();
    let done = run_export(channel, &chosen, file_sink.as_mut(), &options).await;
    drop(file_sink);
    let report = match done {
        Ok(report) => report,
        Err(e) => {
            let _ = std::fs::remove_file(path);
            return Err(CliError::Adapter(e));
        }
    };
    writeln!(
        out,
        "exported samples={} signals={} bytes={} to {path}",
        report.samples, report.signals, report.bytes
    )?;
    Ok(())
}

fn guess(path: &str) -> Result<Format, CliError> {
    let extension = Path::new(path).extension().and_then(|e| e.to_str());
    format(extension.unwrap_or_default()).map_err(|_| usage("import: pass --format F"))
}

/// `import PATH [--format F] [--ack LEVEL] [--batch N]`.
pub(crate) async fn import(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    const USE: &str = "import PATH [--format F] [--ack LEVEL] [--batch N]";
    let Some((path, flags)) = words.split_first() else {
        return Err(usage(USE));
    };
    let mut kind = None;
    let mut options = ImportOptions::default();
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = flags.next().ok_or_else(|| usage(USE))?;
        match *flag {
            "--format" => kind = Some(format(word)?),
            "--ack" => {
                options = options.ack(match *word {
                    "buffered" => AcknowledgementLevel::Buffered,
                    "persisted" => AcknowledgementLevel::Persisted,
                    "durable" => AcknowledgementLevel::Durable,
                    _ => return Err(usage("LEVEL is buffered, persisted or durable")),
                });
            }
            "--batch" => {
                let n = word.parse().map_err(|_| usage("--batch: a number"))?;
                options = options.batch_samples(n);
            }
            _ => return Err(usage(USE)),
        }
    }
    let kind = kind.map_or_else(|| guess(path), Ok)?;
    let file = File::open(path).map_err(io("opening the input"))?;
    let mut file_source = source(kind, file);
    let done = run_import(channel, file_source.as_mut(), &options).await;
    let report = done.map_err(CliError::Adapter)?;
    writeln!(
        out,
        "imported read={} sent={} present={} duplicate={} created={} batches={} throttled={} retried={}",
        report.samples_read,
        report.samples_sent,
        report.samples_present,
        report.samples_duplicate,
        report.resources_created,
        report.batches,
        report.throttled,
        report.retried,
    )?;
    Ok(())
}
