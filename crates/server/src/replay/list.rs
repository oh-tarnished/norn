//! `ListReplaySessions` pages: oldest first, resumed after a named session.

use norn_protocol::common::ErrorReason;
use norn_protocol::replay::{ListReplaySessionsRequest, ListReplaySessionsResponse};
use norn_runtime::sessions::SessionInfo;
use tonic::{Code, Status};

use super::wire::session;
use crate::status::{bad_field, error_info};

const DEFAULT_PAGE: usize = 100;
const MAX_PAGE: usize = 1_000;

/// One page of `all`. The token names the last session given, so a session
/// deleted meanwhile fails it with `ERROR_REASON_INVALID_TOKEN`.
pub(super) fn page(
    all: &[SessionInfo],
    r: &ListReplaySessionsRequest,
) -> Result<ListReplaySessionsResponse, Status> {
    let size = match usize::try_from(r.page_size) {
        Ok(0) => DEFAULT_PAGE,
        Ok(n) => n.min(MAX_PAGE),
        Err(_) => return Err(bad_field("page_size", "must not be negative")),
    };
    let from = if r.page_token.is_empty() {
        0
    } else {
        let after = r.page_token.strip_prefix("r1.");
        let at = after.and_then(|id| all.iter().position(|s| s.id == id));
        at.map(|i| i + 1).ok_or_else(|| {
            let text = "the page token is malformed, or its session has gone";
            error_info(Code::InvalidArgument, ErrorReason::InvalidToken, text, [])
        })?
    };
    let page: Vec<_> = all.iter().skip(from).take(size).collect();
    let more = from + page.len() < all.len();
    let next_page_token = match (more, page.last()) {
        (true, Some(last)) => format!("r1.{}", last.id),
        _ => String::new(),
    };
    Ok(ListReplaySessionsResponse {
        replay_sessions: page.into_iter().map(session).collect(),
        next_page_token,
    })
}
