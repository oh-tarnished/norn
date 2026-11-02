//! `nornctl subscribe`: events, live or replayed, printed as they arrive.

use std::io::Write;

use norn_protocol::replay::ResumeReplaySessionRequest;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::telemetry::telemetry_service_client::TelemetryServiceClient;
use norn_protocol::telemetry::{ClockEventKind, SubscribeEventsRequest, SubscriptionMode, event};
use tonic::transport::Channel;

use crate::CliError;
use crate::query::{print_sample, selection};
use crate::resources::{call, usage};
use crate::session::full;
use crate::time::format;

const USE: &str = "subscribe live TARGET [--select F] [--limit N] \
                   | subscribe replay NAME [--resume] [--limit N]";

fn request(words: &[&str]) -> Result<(SubscribeEventsRequest, bool, Option<i64>), CliError> {
    let (mode, rest) = match words {
        ["live", target, rest @ ..] => {
            let (parent, filter) = selection(target);
            let r = SubscribeEventsRequest {
                mode: SubscriptionMode::Live as i32,
                parent,
                filter,
                ..SubscribeEventsRequest::default()
            };
            (r, rest)
        }
        ["replay", name, rest @ ..] => {
            let r = SubscribeEventsRequest {
                mode: SubscriptionMode::Replay as i32,
                replay_session: full(name),
                ..SubscribeEventsRequest::default()
            };
            (r, rest)
        }
        _ => return Err(usage(USE)),
    };
    let (mut r, mut resume, mut limit) = (mode, false, None);
    let mut flags = rest.iter();
    while let Some(flag) = flags.next() {
        match *flag {
            "--resume" => resume = true,
            "--select" => r.filter = (*flags.next().ok_or_else(|| usage(USE))?).to_owned(),
            "--limit" => {
                let n = flags.next().and_then(|n| n.parse().ok());
                limit = Some(n.ok_or_else(|| usage("--limit: a number"))?);
            }
            _ => return Err(usage(USE)),
        }
    }
    Ok((r, resume, limit))
}

/// Subscribes and prints `#N clock-time` and the event, one a line, until the
/// stream ends, a replay ends or is stopped, or `--limit` events have printed. A
/// replay subscription itself outlives ENDED, since a seek brings more; a command
/// line has nothing to seek with, so it stops there. A state
/// sample after a seek prints as `state` and the sample.
pub(crate) async fn subscribe(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let (request, resume, limit) = request(words)?;
    let name = request.replay_session.clone();
    let m = "TelemetryService.SubscribeEvents";
    let mut client = TelemetryServiceClient::new(channel.clone());
    let mut stream = client
        .subscribe_events(request)
        .await
        .map_err(call(m))?
        .into_inner();
    if resume {
        let r = ResumeReplaySessionRequest {
            name,
            etag: String::new(),
        };
        let mut replays = ReplayServiceClient::new(channel);
        replays
            .resume_replay_session(r)
            .await
            .map_err(call("ReplayService.ResumeReplaySession"))?;
    }
    while let Some(page) = stream.message().await.map_err(call(m))? {
        for e in page.events {
            let t = e
                .clock_time
                .map_or("-".into(), |t| format(t.seconds, t.nanos));
            write!(out, "#{} {t} ", e.event_sequence)?;
            match e.payload {
                Some(event::Payload::Sample(s)) => print_sample(out, s)?,
                Some(event::Payload::StateSample(s)) => {
                    write!(out, "state ")?;
                    print_sample(out, s)?;
                }
                Some(event::Payload::ClockEvent(c)) => {
                    let kind =
                        ClockEventKind::try_from(c.kind).unwrap_or(ClockEventKind::Unspecified);
                    writeln!(
                        out,
                        "{}",
                        kind.as_str_name().trim_start_matches("CLOCK_EVENT_KIND_")
                    )?;
                    if kind == ClockEventKind::Ended {
                        return Ok(());
                    }
                }
                None => writeln!(out, "unknown")?,
            }
            if limit.is_some_and(|l| e.event_sequence >= l) {
                return Ok(());
            }
        }
    }
    Ok(())
}
