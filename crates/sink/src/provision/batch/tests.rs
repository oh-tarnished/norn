//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which item a batch error names: by its details, never its message.

use super::index;
use norn_protocol::rpc::{ErrorDetails, StatusExt};
use std::collections::HashMap;
use tonic::{Code, Status};

fn with_field(field: &str) -> Status {
    let metadata = HashMap::from([("field".to_owned(), field.to_owned())]);
    let details =
        ErrorDetails::with_error_info("ERROR_REASON_INVALID_ARGUMENT", "norn.local", metadata);
    Status::with_error_details(Code::InvalidArgument, "bad", details)
}

#[test]
fn a_batch_error_names_the_item_it_refused_in_its_error_info() {
    assert_eq!(index(&with_field("requests[17].signal_id")), Some(17));
    assert_eq!(index(&with_field("requests[0].parent")), Some(0));
}

#[test]
fn a_bad_request_violation_names_it_too() {
    let mut details = ErrorDetails::new();
    details.add_bad_request_violation("requests[3].entity_id", "must be a slug");
    let status = Status::with_error_details(Code::InvalidArgument, "bad", details);
    assert_eq!(index(&status), Some(3));
}

#[test]
fn an_error_naming_no_item_names_none_and_a_message_is_never_read() {
    assert_eq!(index(&with_field("requests")), None);
    assert_eq!(index(&with_field("requests[x]")), None);
    let bare = Status::invalid_argument("requests[17].signal_id: must be a slug");
    assert_eq!(index(&bare), None);
}
