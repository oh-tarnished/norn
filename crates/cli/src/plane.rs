//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `--shared-memory`: `append` and `import` over the iceoryx2 data plane when
//! the daemon offers it, for comparing it with the socket.

use std::sync::Arc;

use norn_ipc::AppendOptions;
use norn_shared_memory::SharedMemory;

/// The flag. It takes no value, so it is taken out before the pairs are read.
pub(crate) const FLAG: &str = "--shared-memory";

/// The append options `flags` ask for, and the flags without [`FLAG`]. Without
/// it, gRPC only. With it, a ring when the daemon grants one, and gRPC
/// otherwise: the answers are the same, so the flag never changes what is stored.
pub(crate) fn options<'a>(flags: &[&'a str]) -> (AppendOptions, Vec<&'a str>) {
    let wanted = flags.contains(&FLAG);
    let rest = flags.iter().copied().filter(|f| *f != FLAG).collect();
    let options = AppendOptions::default();
    if wanted {
        (options.shared_memory(Arc::new(SharedMemory)), rest)
    } else {
        (options, rest)
    }
}

/// Whether `flags` ask for shared memory.
pub(crate) fn wanted(flags: &[&str]) -> bool {
    flags.contains(&FLAG)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_flag_is_taken_out_and_the_pairs_are_kept_in_order() {
        let (_, rest) = options(&["--ack", "buffered", FLAG, "--at", "now"]);
        assert_eq!(rest, ["--ack", "buffered", "--at", "now"]);
        assert!(wanted(&[FLAG]));
        assert!(!wanted(&["--ack", "durable"]));
    }
}
