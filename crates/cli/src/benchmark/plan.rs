//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The `nornctl benchmark` command line.

use std::time::Duration;

use norn_protocol::telemetry::AcknowledgementLevel;

use crate::CliError;
use crate::resources::usage;

const USAGE: &str = "benchmark [--stream ID] [--signals N] [--rate N|max] [--duration D] \
     [--batch N] [--ack LEVEL] [--queries N] [--allow-non-empty]";

/// The stream, and the entity holding its signals, unless `--stream` names another.
const DEFAULT_STREAM: &str = "nornctl-benchmark";

/// What to run.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Plan {
    pub stream: String,
    pub signals: usize,
    /// Samples per second over all signals; `None` as fast as acknowledged.
    pub rate: Option<u64>,
    pub duration: Duration,
    pub batch: Option<usize>,
    pub ack: AcknowledgementLevel,
    pub queries: usize,
    pub allow_non_empty: bool,
}

fn positive<T: std::str::FromStr + Default + PartialOrd>(
    flag: &str,
    word: &str,
) -> Result<T, CliError> {
    word.parse()
        .ok()
        .filter(|n| *n > T::default())
        .ok_or_else(|| usage(&format!("{flag}: a positive number")))
}

pub(crate) fn plan(words: &[&str]) -> Result<Plan, CliError> {
    let mut plan = Plan {
        stream: DEFAULT_STREAM.to_owned(),
        signals: 10,
        rate: Some(10_000),
        duration: Duration::from_secs(5),
        batch: None,
        ack: AcknowledgementLevel::Persisted,
        queries: 10,
        allow_non_empty: false,
    };
    let mut words = words.iter();
    while let Some(flag) = words.next() {
        if *flag == "--allow-non-empty" {
            plan.allow_non_empty = true;
            continue;
        }
        let word = *words.next().ok_or_else(|| usage(USAGE))?;
        match *flag {
            "--stream" => word
                .strip_prefix("streams/")
                .unwrap_or(word)
                .clone_into(&mut plan.stream),
            "--signals" => plan.signals = positive(flag, word)?,
            "--rate" if word == "max" => plan.rate = None,
            "--rate" => plan.rate = Some(positive(flag, word)?),
            "--duration" => {
                let nanos = crate::time::duration(word).filter(|n| *n > 0);
                let nanos = nanos.ok_or_else(|| usage("--duration is like 500ms, 10s, 5m"))?;
                plan.duration = Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX));
            }
            "--batch" => plan.batch = Some(positive(flag, word)?),
            "--ack" => {
                plan.ack = match word {
                    "buffered" => AcknowledgementLevel::Buffered,
                    "persisted" => AcknowledgementLevel::Persisted,
                    "durable" => AcknowledgementLevel::Durable,
                    _ => return Err(usage("LEVEL is buffered, persisted or durable")),
                };
            }
            "--queries" => {
                plan.queries = word.parse().map_err(|_| usage("--queries: a number"))?;
            }
            _ => return Err(usage(USAGE)),
        }
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_defaults_use_their_own_stream_and_refuse_a_non_empty_one() {
        let p = plan(&[]).ok();
        let p = p.as_ref();
        assert_eq!(p.map(|p| p.stream.as_str()), Some(DEFAULT_STREAM));
        assert_eq!(p.map(|p| p.allow_non_empty), Some(false));
    }

    #[test]
    fn flags_parse_and_nonsense_does_not() {
        let words = [
            "--rate",
            "max",
            "--signals",
            "3",
            "--duration",
            "2s",
            "--allow-non-empty",
        ];
        let p = plan(&words).ok();
        assert_eq!(p.as_ref().and_then(|p| p.rate), None);
        assert_eq!(p.as_ref().map(|p| p.signals), Some(3));
        assert_eq!(p.as_ref().map(|p| p.duration), Some(Duration::from_secs(2)));
        assert_eq!(p.map(|p| p.allow_non_empty), Some(true));
        assert!(plan(&["--signals", "0"]).is_err());
        assert!(plan(&["--rate", "-1"]).is_err());
        assert!(plan(&["--duration", "soon"]).is_err());
        assert!(plan(&["--ack"]).is_err());
        assert!(plan(&["--nonsense", "1"]).is_err());
    }
}
