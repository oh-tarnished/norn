//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Resource names ↔ the IDs inside them. Borrows from the name it is given.

use tonic::Status;

use crate::status::bad_field;

/// The `{stream}` of `streams/{stream}`.
///
/// # Errors
/// `INVALID_ARGUMENT` for any other shape.
pub(crate) fn stream(name: &str) -> Result<&str, Status> {
    name.strip_prefix("streams/")
        .filter(|s| !s.is_empty() && !s.contains('/'))
        .ok_or_else(|| bad_field("name", "must be streams/{stream}"))
}

/// The `{entity}` of `entities/{entity}`.
///
/// # Errors
/// `INVALID_ARGUMENT` for any other shape.
pub(crate) fn entity(name: &str) -> Result<&str, Status> {
    name.strip_prefix("entities/")
        .filter(|s| !s.is_empty() && !s.contains('/'))
        .ok_or_else(|| bad_field("name", "must be entities/{entity}"))
}

/// The `({entity}, {signal})` of `entities/{entity}/signals/{signal}`.
///
/// # Errors
/// `INVALID_ARGUMENT` for any other shape.
pub(crate) fn signal(name: &str) -> Result<(&str, &str), Status> {
    name.strip_prefix("entities/")
        .and_then(|rest| rest.split_once("/signals/"))
        .filter(|(e, s)| !e.is_empty() && !s.is_empty() && !e.contains('/') && !s.contains('/'))
        .ok_or_else(|| bad_field("name", "must be entities/{entity}/signals/{signal}"))
}
