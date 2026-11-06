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
  daemon's user, or a named pipe whose protected DACL grants only that user. CI runs
  the daemon and `nornctl` over a named pipe on Windows, and checks that the descriptor
  names only the current user; refusing another user is not tested. The
  daemon refuses a more open directory, or a symbolic link, for its own socket and
  for every MCP socket. Loopback TCP exists for tests and has no OS identity check.
  [docs/reference/ipc.md](docs/reference/ipc.md#security-model-spec-41) has the model.
- **Bounded resources.** Connections, in-flight batches, batch size, metadata and
  resource counts have explicit limits, and decode memory is charged to a budget, so
  a client gets a refusal rather than exhausting the daemon.
- **An owner-only database directory.** Created `0700`, or tightened to it at start
  (Unix), since files inside are created under the umask.
- **Two opt-in listeners are open to every local user.** `listen = "tcp:..."` and MCP
  over HTTP (`--mcp http:...` or an `http:` transport) have no identity check and no
  authentication. Once an operator turns either on, any program of any user on the
  machine can use it: TCP gives full read and write through the gRPC API, MCP over
  HTTP reads all history through the tools. The daemon keeps this behaviour by the
  owner's decision and only logs a warning at every start
  ([review](docs/security/review-2026-11.md#owner-decision-f4-and-f5), F4 and F5).
  Neither is a default. The safe alternatives: for gRPC, the default Unix socket in
  an owner-only directory or, on Windows, a named pipe with an owner-only security
  descriptor; for MCP, `stdio`, a Unix socket or a named pipe. MCP over HTTP also
  refuses a browser `Origin` that is not loopback or a configured host.
- **Opt-in encryption at rest, experimental in 1.0.** ChaCha20-Poly1305 per chunk,
  pure Rust; it covers sample data and journals, not names, time ranges or the metadata
  log ([docs/reference/storage.md](docs/reference/storage.md#encryption)). It has had
  no independent review: the only review of it is by its author. Its on-disk form may
  change within 1.x, so it is excluded from the
  [compatibility promise](docs/reference/compatibility.md#what-1x-promises). A backup of
  an encrypted store that holds no segment yet does not record that it was encrypted,
  so restored and started without a key it opens as plain
  ([storage](docs/reference/storage.md#on-disk)).
- **Checksummed storage.** Damage is detected and reported, never silently
  discarded ([docs/reference/storage.md](docs/reference/storage.md#recovery)).
- **Decoders never panic** on malformed input, and allocate only what their input
  implies. Value decoding is property-tested on arbitrary input.
- **Supply chain.** `cargo-deny` checks advisories, licences and sources in CI, and
  `xtask deps` keeps C out of the graph.

## Shared memory (from 1.0.0-rc.1, off by default)

The iceoryx2 data plane ([docs/reference/shared-memory.md](docs/reference/shared-memory.md))
arrived with Norn 1.0.0-rc.1 and protocol 1.7. It is off unless
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

There is no external audit, and no independent review of encryption at rest. No test
cuts the power: crash tests kill the process. There is no disk budget (review L1). On
Windows, the database directory relies on its inherited ACL, which has not been
checked to be owner-only.
