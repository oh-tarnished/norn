//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Naming, pacing and printing replay sessions for `nornctl replay`.

use std::io::Write;

use norn_protocol::replay::replay_session::Pace;
use norn_protocol::replay::{ReplaySession, ReplayState};

use crate::CliError;
use crate::resources::usage;
use crate::time::format;

/// `replaySessions/{id}` for a bare id or a full name.
pub(crate) fn full(name: &str) -> String {
    if name.starts_with("replaySessions/") {
        name.to_owned()
    } else {
        format!("replaySessions/{name}")
    }
}

/// A pace from `max`, or a multiple such as `10`, `10x` or `0.5`.
pub(crate) fn pace(word: &str) -> Result<Pace, CliError> {
    if word == "max" {
        return Ok(Pace::MaxSpeed(true));
    }
    let x = word.strip_suffix('x').unwrap_or(word).parse::<f64>();
    x.map(Pace::Speed)
        .map_err(|_| usage("a speed is a number like 10 or 0.5, or max"))
}

fn show_pace(p: Option<Pace>) -> String {
    match p {
        Some(Pace::MaxSpeed(true)) => "max".to_owned(),
        Some(Pace::Speed(x)) => format!("{x}x"),
        _ => "1x".to_owned(),
    }
}

/// One line: name, state, clock, events emitted, pace and interval.
pub(crate) fn print(out: &mut impl Write, s: &ReplaySession) -> std::io::Result<()> {
    let state = ReplayState::try_from(s.state).unwrap_or(ReplayState::Unspecified);
    let state = state.as_str_name().trim_start_matches("REPLAY_STATE_");
    let t =
        |t: Option<prost_types::Timestamp>| t.map_or("-".into(), |t| format(t.seconds, t.nanos));
    let interval = s.interval.unwrap_or_default();
    writeln!(
        out,
        "{} {state} at={} emitted={} pace={} from={} to={}",
        s.name,
        t(s.current_time),
        s.emitted_count,
        show_pace(s.pace),
        t(interval.start_time),
        t(interval.end_time),
    )
}
