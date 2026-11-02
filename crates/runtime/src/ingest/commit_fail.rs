//! When a stream's journal fails.

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;

use super::commit::Committer;
use super::decision::AckFailure;
use super::shared::Work;

impl Committer {
    /// Refuses the stream and tells every waiting batch. What was journalled
    /// stays for recovery; budget held for it is returned.
    pub(super) fn fail(&mut self, error: &str, rx: &Receiver<Work>) {
        tracing::error!(stream = self.shared.stream.get(), error, "commit failed");
        self.shared.failed.store(true, Ordering::Release);
        let failure: AckFailure = Arc::from(error);
        let pending = self.unflushed.drain(..).chain(self.unsynced.drain(..));
        for (_, ack) in pending {
            let _ = ack.send(Err(Arc::clone(&failure)));
        }
        // Sealed, not dropped: these were acknowledged, so queries keep seeing them
        // until restart moves them from the journal into segments. Their memory
        // stays reserved for that long, against a stream that takes no more.
        let frozen = self.shared.tail.seal(Vec::new()).frozen;
        let bytes = frozen.memory.as_ref().map_or(0, |m| m.bytes());
        self.shared.held.fetch_sub(bytes, Ordering::AcqRel);
        self.store = None;
        while let Ok(work) = rx.recv() {
            self.shared.queued.fetch_sub(1, Ordering::AcqRel);
            self.shared
                .held
                .fetch_sub(work.memory.bytes(), Ordering::AcqRel);
            if let Some(ack) = work.ack {
                let _ = ack.send(Err(Arc::clone(&failure)));
            }
        }
    }
}
