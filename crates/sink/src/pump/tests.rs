//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which ends of a call are Norn draining, by reason and never by message.

use super::{End, draining, ended};
use norn_protocol::rpc::{ErrorDetails, StatusExt};
use std::collections::HashMap;
use tonic::{Code, Status};

fn with_reason(code: Code, reason: &str) -> Status {
    let details = ErrorDetails::with_error_info(reason, "norn.local", HashMap::new());
    Status::with_error_details(code, "the daemon is stopping", details)
}

#[test]
fn unavailable_shutting_down_is_a_drain() {
    let status = with_reason(Code::Unavailable, "ERROR_REASON_SHUTTING_DOWN");
    assert!(draining(&status));
    assert!(matches!(ended(&status), End::Draining));
}

#[test]
fn unavailable_for_another_reason_or_none_is_norn_lost() {
    let other = with_reason(Code::Unavailable, "ERROR_REASON_STORAGE_UNAVAILABLE");
    assert!(!draining(&other));
    let bare = Status::unavailable("ERROR_REASON_SHUTTING_DOWN");
    assert!(!draining(&bare), "a message is never matched");
    assert!(matches!(ended(&bare), End::Lost(_)));
}

#[test]
fn shutting_down_under_another_code_is_not_a_drain() {
    let status = with_reason(Code::Internal, "ERROR_REASON_SHUTTING_DOWN");
    assert!(!draining(&status));
}
