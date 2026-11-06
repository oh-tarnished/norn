//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The registry's signals.

use super::{COMMIT, Registry, no};
use norn_protocol::signal::signal_service_server::SignalService;
use norn_protocol::signal::{self, Signal};
use prost::Message as _;
use std::sync::atomic::Ordering;
use tonic::{Request, Response, Status, async_trait};

#[async_trait]
impl SignalService for Registry {
    async fn create_signal(
        &self,
        _: Request<signal::CreateSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        self.singly.fetch_add(1, Ordering::SeqCst);
        Err(Status::internal("a batch create was expected"))
    }
    async fn batch_create_signals(
        &self,
        request: Request<signal::BatchCreateSignalsRequest>,
    ) -> Result<Response<signal::BatchCreateSignalsResponse>, Status> {
        self.within(request.get_ref().encoded_len())?;
        tokio::time::sleep(COMMIT).await;
        let requests = request.into_inner().requests;
        self.batches.fetch_add(1, Ordering::SeqCst);
        self.largest.fetch_max(requests.len(), Ordering::SeqCst);
        let signals: Vec<Signal> = requests
            .into_iter()
            .map(|r| {
                let mut s = r.signal.unwrap_or_default();
                s.name = format!("{}/signals/{}", r.parent, r.signal_id);
                s
            })
            .collect();
        let mut made = self.made.lock().unwrap();
        made.extend(signals.iter().map(|s| s.name.clone()));
        Ok(Response::new(signal::BatchCreateSignalsResponse {
            signals,
        }))
    }
    async fn get_signal(
        &self,
        _: Request<signal::GetSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        no()
    }
    async fn list_signals(
        &self,
        _: Request<signal::ListSignalsRequest>,
    ) -> Result<Response<signal::ListSignalsResponse>, Status> {
        no()
    }
    async fn update_signal(
        &self,
        _: Request<signal::UpdateSignalRequest>,
    ) -> Result<Response<Signal>, Status> {
        no()
    }
    async fn delete_signal(
        &self,
        _: Request<signal::DeleteSignalRequest>,
    ) -> Result<Response<()>, Status> {
        no()
    }
}
