//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Keeps a panic in a third-party format crate from ending an import.
//!
//! A file is untrusted input, and the `arrow`, `parquet` and `mcap` crates assume a
//! well-formed one in places: a hostile length in an Arrow buffer reaches
//! `Buffer::slice`, and one in an MCAP chunk underflows a counter. That is damage to
//! report as an error naming the file, not a crash. Only calls into those crates
//! run under this; Norn's own code keeps its panics.

use std::any::Any;
use std::panic::{AssertUnwindSafe, catch_unwind};

/// What a panic said.
fn message(payload: &(dyn Any + Send)) -> String {
    let text = payload
        .downcast_ref::<&str>()
        .copied()
        .or_else(|| payload.downcast_ref::<String>().map(String::as_str));
    format!("the reader gave up on it: {}", text.unwrap_or("a panic"))
}

/// Runs `read`, turning a panic into its message. The caller must abandon what
/// `read` was using afterwards: a source that returned an error is not read again.
///
/// # Errors
/// What the panic said.
pub fn contained<T>(read: impl FnOnce() -> T) -> Result<T, String> {
    catch_unwind(AssertUnwindSafe(read)).map_err(|p| message(p.as_ref()))
}

#[cfg(test)]
mod tests {
    #![allow(clippy::panic)]

    use super::contained;

    #[test]
    fn a_value_passes_through() {
        assert_eq!(contained(|| 7), Ok(7));
    }

    #[test]
    fn a_panic_becomes_its_message() {
        let got: Result<(), String> = contained(|| panic!("a bad length"));
        assert_eq!(
            got,
            Err("the reader gave up on it: a bad length".to_owned())
        );
    }
}
