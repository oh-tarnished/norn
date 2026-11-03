<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Security

## Reporting

Report a vulnerability privately to the repository owner through GitHub's
private vulnerability reporting, not a public issue. Expect an
acknowledgement before any fix is discussed in public.

## What exists today

- **Local IPC only.** A Unix socket inside an owner-only (0700) directory, or a
  named pipe whose code has not yet been run on Windows. The daemon refuses a more
  open directory. Loopback TCP exists for tests and has no OS identity check.
  [docs/reference/ipc.md](docs/reference/ipc.md#security-model-spec-41) has the model.
- **Bounded resources.** Connections, in-flight batches, batch size, metadata and
  resource counts have explicit limits, and decode memory is charged to a budget, so
  a client gets a refusal rather than exhausting the daemon.
- **Checksummed storage.** Damage is detected and reported, never silently
  discarded ([docs/reference/storage.md](docs/reference/storage.md#recovery)).
- **Decoders never panic** on malformed input, and allocate only what their input
  implies. Value decoding is property-tested on arbitrary input.
- **Supply chain.** `cargo-deny` checks advisories, licences and sources in CI, and
  `xtask deps` keeps C out of the graph.

## Shared memory (1.0.0-rc.1, off by default)

The iceoryx2 data plane ([docs/reference/shared-memory.md](docs/reference/shared-memory.md))
arrives with Norn 1.0.0-rc.1 and protocol 1.7. It is off unless
`[shared_memory] enabled = true`, offered only on a Unix socket endpoint, whose `0700`
directory proves the client runs as the daemon's user, and never on Windows, where
iceoryx2 0.10 cannot make a mapping owner-only.

**Trust boundary: the daemon's user.** iceoryx2's files live in an owner-only
directory inside the socket directory, and every shared-memory object is `0600`, so
another user cannot open a ring. Any process running as the daemon's user can, and
iceoryx2 trusts what it finds in its shared bookkeeping: a misbehaving or malicious
process of that user can crash the daemon by writing there.

What Norn does about it: rings are off by default; each frame is length-checked
against `max_frame_bytes` before it is read, paid for from `ipc_bytes`, copied once and
decoded only from the copy; a writer that dies mid-frame publishes nothing; the number
and size of rings are bounded. What it cannot do is protect the daemon from a same-user
process that writes into iceoryx2's bookkeeping. Such a process could equally signal
the daemon or edit its files, so enabling rings does not widen who can harm Norn, only
how; but it turns a bug in any program of that user into a possible daemon crash.
Enable shared memory only where every process of that user is trusted.

**Unsafe.** `norn-shared-memory` is `#![forbid(unsafe_code)]`, and Norn adds no
`unsafe` for shared memory. iceoryx2's own `unsafe` is not audited by this project.

## Not done yet

No security review has been carried out, and there is no fuzzing (stage 16). Limits
on queries, replay sessions and subscriptions wait for those subsystems. Data at
rest is not encrypted.
