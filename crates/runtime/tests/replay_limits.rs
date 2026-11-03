//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Replay session limits, names, etags, expiry and stopping.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

#[path = "../../query/tests/common/mod.rs"]
mod common;
mod replays_common;

use std::time::Duration;

use norn_core::Config;
use norn_replay::{Control, Pace, ReplayError};
use norn_runtime::End;
use replays_common::{config, id, setup, spec};

#[tokio::test(flavor = "multi_thread")]
async fn stopping_ends_every_subscriber_with_stopped() {
    let s = setup(10, Config::default());
    let id = id(&s, Pace::default(), 10);
    let mut sub = s.replays.subscribe(&id).await.unwrap();
    s.replays.control(&id, None, Control::Stop).await.unwrap();
    assert!(matches!(sub.next().await, Err(End::Stopped(_))));
    let refused = s.replays.subscribe(&id).await.unwrap_err();
    assert!(matches!(refused, ReplayError::Conflict { .. }));
    s.replays.delete(&id, None).await.unwrap();
    assert!(matches!(s.replays.get(&id), Err(ReplayError::NotFound)));
    s.cancel.cancel();
    s.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn limits_and_names_are_refused_with_typed_errors() {
    let s = setup(10, config(2, Duration::from_secs(60)));
    let labels = || ("entities/e".to_owned(), String::new());
    s.replays
        .create(Some("a".into()), spec(10, Pace::Maximum), labels())
        .unwrap();
    let again = s
        .replays
        .create(Some("a".into()), spec(10, Pace::Maximum), labels());
    assert!(matches!(again, Err(ReplayError::AlreadyExists)));
    s.replays
        .create(None, spec(10, Pace::Maximum), labels())
        .unwrap();
    let third = s.replays.create(None, spec(10, Pace::Maximum), labels());
    assert!(matches!(
        third,
        Err(ReplayError::TooManySessions { limit: 2 })
    ));
    let info = s.replays.get("a").unwrap();
    let stale = Some(info.view.generation + 1);
    let refused = s
        .replays
        .control("a", stale, Control::Resume)
        .await
        .unwrap_err();
    assert!(matches!(refused, ReplayError::EtagMismatch));
    s.replays
        .control("a", Some(info.view.generation), Control::Resume)
        .await
        .unwrap();
    s.cancel.cancel();
    s.task.await.unwrap();
}

#[tokio::test(flavor = "multi_thread")]
async fn an_idle_session_expires_unless_someone_is_subscribed() {
    let s = setup(10, config(4, Duration::from_secs(60)));
    let idle = id(&s, Pace::default(), 10);
    let watched = id(&s, Pace::default(), 10);
    let _sub = s.replays.subscribe(&watched).await.unwrap();
    for _ in 0..8 {
        while s.clock.sleepers() == 0 {
            tokio::task::yield_now().await;
        }
        s.clock.advance(Duration::from_secs(15));
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(matches!(s.replays.get(&idle), Err(ReplayError::NotFound)));
    assert!(s.replays.get(&watched).is_ok());
    s.cancel.cancel();
    s.task.await.unwrap();
}
