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

- **Local IPC only.** A Unix socket inside an owner-only (0700) directory of the
  daemon's user, or a named pipe whose code has not yet been run on Windows. The
  daemon refuses a more open directory, or a symbolic link, for its own socket and
  for every MCP socket. Loopback TCP exists for tests and has no OS identity check.
  [docs/reference/ipc.md](docs/reference/ipc.md#security-model-spec-41) has the model.
- **Bounded resources.** Connections, in-flight batches, batch size, metadata and
  resource counts have explicit limits, and decode memory is charged to a budget, so
  a client gets a refusal rather than exhausting the daemon.
- **An owner-only database directory.** Created `0700`, or tightened to it at start
  (Unix), since files inside are created under the umask.
- **Listeners every local user can reach are named.** `listen = "tcp:..."` and MCP
  over HTTP have no identity check or authentication; the daemon logs a warning at
  every start when either is on. MCP over HTTP refuses a browser `Origin` that is
  not loopback or a configured host.
- **Opt-in encryption at rest.** ChaCha20-Poly1305 per chunk, pure Rust; it covers
  sample data and journals, not names, time ranges or the metadata log
  ([docs/reference/storage.md](docs/reference/storage.md#encryption)).
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

## Fuzzing

Every decoder that sees untrusted bytes has a cargo-fuzz target in `fuzz/`: the
codecs, the storage reader and recovery, the index's files, the journal, the
metadata log, requests through the server's metering and validation, shared-memory
frames, filters and page tokens, and the CSV, Parquet, Arrow and MCAP readers. Each
asserts no panic, a bound on the heap it holds and, where an encoder exists, a
round trip. `just fuzz [SECONDS] [TARGET]` runs them, and
[a nightly workflow](.github/workflows/fuzz.yml) runs each for a bounded time and
keeps any crash as an artifact. A fuzz run is evidence about the inputs it tried, not
a proof.

The `arrow`, `parquet` and `mcap` crates can panic on a hostile file. Norn catches
that during an import and reports it as damage, and checks the lengths those crates
would otherwise reserve before calling them; a panic message still reaches stderr.

One bound is not strict: a request's decoding reservation is capped at half of
`ipc_bytes`, so with the default 32 MiB a 256 KiB batch create of empty elements can
briefly hold more than it reserved, up to about 50 MiB.

## Review

A first source review is [docs/security/review-2026-11.md](docs/security/review-2026-11.md).
It lists what was examined, what was not, the medium findings fixed, and those still
open: chiefly that TCP and HTTP listeners, once turned on, are open to every local
user (warned of, not closed), and that there is no disk budget.

## Not done yet

There is no external audit, and Windows has not been run. Limits on queries, replay
sessions and subscriptions wait for those subsystems.
