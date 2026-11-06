//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Many futures at once on the task that awaits them, spawning nothing: so a
//! session ending mid-chunk drops every call of the chunk with it, and nothing
//! the sink started outlives [`Sink::run`](crate::Sink::run).
//!
//! Each wake polls every unfinished future, which is fine for the at most a
//! few hundred calls of one chunk, each a round trip to the daemon.

use std::future::{Future, poll_fn};
use std::pin::Pin;
use std::task::Poll;

/// Every output of `futures`, in their order.
pub(crate) async fn join_all<F: Future>(futures: Vec<F>) -> Vec<F::Output> {
    let mut running: Vec<Option<Pin<Box<F>>>> =
        futures.into_iter().map(|f| Some(Box::pin(f))).collect();
    let mut outputs: Vec<Option<F::Output>> = running.iter().map(|_| None).collect();
    poll_fn(|cx| {
        let mut done = true;
        for (slot, output) in running.iter_mut().zip(outputs.iter_mut()) {
            let Some(future) = slot else { continue };
            match future.as_mut().poll(cx) {
                Poll::Ready(value) => {
                    *output = Some(value);
                    *slot = None;
                }
                Poll::Pending => done = false,
            }
        }
        if done { Poll::Ready(()) } else { Poll::Pending }
    })
    .await;
    outputs.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::join_all;
    use std::time::Duration;

    #[tokio::test]
    async fn every_output_comes_back_in_the_order_given_whatever_order_they_finish() {
        let waits = [30_u64, 10, 20];
        let futures = waits.iter().map(|ms| async move {
            tokio::time::sleep(Duration::from_millis(*ms)).await;
            *ms
        });
        assert_eq!(join_all(futures.collect()).await, waits);
        assert!(
            join_all(Vec::<std::future::Ready<()>>::new())
                .await
                .is_empty()
        );
    }
}
