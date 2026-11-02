//! A replay set-up on a fake clock, shared by the session tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::sync::Arc;
use std::time::Duration;

use norn_core::{Config, Limits, Value, ValueKind};
use norn_query::QueryLimits;
use norn_replay::{Batch, Pace, Payload, SessionSpec};
use norn_runtime::sessions::Replays;
use norn_runtime::{Clock, FakeClock, Subscription};
use tokio_util::sync::CancellationToken;

use super::common::{Scratch, at, rig, sample, store, target};

pub fn config(sessions: usize, idle: Duration) -> Config {
    let mut limits = Limits::default();
    limits.max_replay_sessions = sessions;
    limits.replay_session_idle_timeout = idle;
    Config::builder().limits(limits).build().unwrap()
}

pub fn spec(end: i64, pace: Pace) -> SessionSpec {
    SessionSpec {
        start: at(0),
        end: at(end),
        targets: vec![target(0, ValueKind::F64)],
        pace,
    }
}

pub struct Setup {
    pub _scratch: Scratch,
    pub replays: Arc<Replays>,
    pub clock: FakeClock,
    pub cancel: CancellationToken,
    pub task: tokio::task::JoinHandle<()>,
}

pub fn setup(samples: i64, config: Config) -> Setup {
    let s = Scratch::new();
    store(
        &s.dir(),
        &[(0..samples)
            .map(|t| sample(0, t, Value::F64(t as f64)))
            .collect()],
        64,
    );
    let r = rig(&s.0, 1 << 30, QueryLimits::default());
    let clock = FakeClock::new(at(1_000));
    let shared: Arc<dyn Clock> = Arc::new(clock.clone());
    let replays = Arc::new(Replays::new(r.engine, &config, shared));
    let cancel = CancellationToken::new();
    let (run, token) = (Arc::clone(&replays), cancel.clone());
    let task = tokio::spawn(async move { run.run(token).await });
    Setup {
        _scratch: s,
        replays,
        clock,
        cancel,
        task,
    }
}

pub fn id(setup: &Setup, pace: Pace, end: i64) -> String {
    let labels = ("entities/e".to_owned(), String::new());
    setup
        .replays
        .create(None, spec(end, pace), labels)
        .unwrap()
        .id
}

pub async fn events(sub: &mut Subscription<Arc<Batch>>, until: usize) -> Vec<Payload> {
    let mut out = Vec::new();
    while out.len() < until {
        let batch = sub.next().await.unwrap();
        out.extend(batch.events.iter().map(|e| e.payload.clone()));
    }
    out
}
