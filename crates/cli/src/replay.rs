//! `nornctl replay`: sessions and their clocks, over ReplayService.

use std::io::Write;

use norn_protocol::google::r#type::Interval;
use norn_protocol::replay::replay_service_client::ReplayServiceClient;
use norn_protocol::replay::step_replay_session_request::Step;
use norn_protocol::replay::{
    CreateReplaySessionRequest, DeleteReplaySessionRequest, GetReplaySessionRequest,
    ListReplaySessionsRequest, PauseReplaySessionRequest, ReplaySession,
    ResumeReplaySessionRequest, SeekReplaySessionRequest, StepReplaySessionRequest,
    StopReplaySessionRequest, UpdateReplaySessionRequest,
};
use tonic::transport::Channel;

use crate::CliError;
use crate::query::{selection, timestamp, window};
use crate::resources::{call, usage};
use crate::session::{full, pace, print};

const USE: &str = "replay create TARGET --from T --to T [--speed X|max] [--id ID] [--select F] \
                   | list | get|pause|resume|stop|delete NAME | seek NAME T \
                   | step NAME N|D | speed NAME X|max";

fn create(words: &[&str]) -> Result<CreateReplaySessionRequest, CliError> {
    let Some((target, flags)) = words.split_first() else {
        return Err(usage(USE));
    };
    let (scope, filter) = selection(target);
    let mut s = ReplaySession {
        scope,
        filter,
        ..ReplaySession::default()
    };
    let (mut interval, mut id) = (Interval::default(), String::new());
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let word = *flags.next().ok_or_else(|| usage("a flag needs a value"))?;
        match *flag {
            "--from" => interval.start_time = Some(timestamp(word)?),
            "--to" => interval.end_time = Some(timestamp(word)?),
            "--speed" => s.pace = Some(pace(word)?),
            "--id" => id = word.to_owned(),
            "--select" => s.filter = word.to_owned(),
            _ => return Err(usage(USE)),
        }
    }
    s.interval = Some(interval);
    Ok(CreateReplaySessionRequest {
        replay_session_id: id,
        replay_session: Some(s),
    })
}

fn step(word: &str) -> Result<Step, CliError> {
    if let Ok(n) = word.parse::<i32>() {
        return Ok(Step::EventCount(n));
    }
    window(word).map(Step::Duration)
}

/// Runs a `replay` command and prints the session after it.
pub(crate) async fn replay(
    channel: Channel,
    words: &[&str],
    out: &mut impl Write,
) -> Result<(), CliError> {
    let mut c = ReplayServiceClient::new(channel);
    let m = "ReplayService";
    let session = match words {
        ["create", rest @ ..] => c.create_replay_session(create(rest)?).await,
        ["list"] => {
            let mut page_token = String::new();
            loop {
                let r = ListReplaySessionsRequest {
                    page_size: 0,
                    page_token,
                };
                let page = c
                    .list_replay_sessions(r)
                    .await
                    .map_err(call(m))?
                    .into_inner();
                for s in &page.replay_sessions {
                    print(out, s)?;
                }
                if page.next_page_token.is_empty() {
                    return Ok(());
                }
                page_token = page.next_page_token;
            }
        }
        ["get", n] => {
            c.get_replay_session(GetReplaySessionRequest { name: full(n) })
                .await
        }
        ["pause", n] => {
            c.pause_replay_session(PauseReplaySessionRequest {
                name: full(n),
                etag: String::new(),
            })
            .await
        }
        ["resume", n] => {
            c.resume_replay_session(ResumeReplaySessionRequest {
                name: full(n),
                etag: String::new(),
            })
            .await
        }
        ["stop", n] => {
            c.stop_replay_session(StopReplaySessionRequest {
                name: full(n),
                etag: String::new(),
            })
            .await
        }
        ["delete", n] => {
            let r = DeleteReplaySessionRequest {
                name: full(n),
                etag: String::new(),
            };
            c.delete_replay_session(r).await.map_err(call(m))?;
            return Ok(writeln!(out, "deleted {}", full(n))?);
        }
        ["seek", n, t] => {
            let r = SeekReplaySessionRequest {
                name: full(n),
                seek_time: Some(timestamp(t)?),
                etag: String::new(),
            };
            c.seek_replay_session(r).await
        }
        ["step", n, by] => {
            let r = StepReplaySessionRequest {
                name: full(n),
                step: Some(step(by)?),
                etag: String::new(),
            };
            c.step_replay_session(r).await
        }
        ["speed", n, x] => {
            let s = ReplaySession {
                name: full(n),
                pace: Some(pace(x)?),
                ..ReplaySession::default()
            };
            let r = UpdateReplaySessionRequest {
                replay_session: Some(s),
                update_mask: None,
            };
            c.update_replay_session(r).await
        }
        _ => return Err(usage(USE)),
    };
    let session = session.map_err(call(m))?.into_inner();
    Ok(print(out, &session)?)
}
