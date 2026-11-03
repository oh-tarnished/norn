<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Changelog

## Unreleased (0.2.0)

Protocol 1.7. Built and tested; not yet released.

### Shared memory

- An opt-in shared-memory data plane, `norn-shared-memory`, over iceoryx2 0.10:
  `AppendSamples` batches travel through a bounded ring while the socket stays the
  control plane and carries every acknowledgement. Off by default
  (`[shared_memory] enabled = false`), offered only on Unix sockets and never on
  Windows. Any call without a ring falls back to the socket transparently, with the
  same acknowledgements, backpressure and errors.
- Protocol 1.7: `SharedMemoryPlane`, `NegotiateProtocolResponse.shared_memory`, and the
  metadata keys `norn-data-plane` and `norn-shared-memory-service`.
- `nornctl append --shared-memory` (prints `plane SHARED_MEMORY` when a ring carried
  the sample), `nornctl import --shared-memory`, and `import --timing`.
- Trust boundary: a process of the daemon's own user can crash the daemon through
  iceoryx2's shared bookkeeping, which is why it is off by default. See
  [SECURITY.md](../../SECURITY.md).
- `deny.toml` allows 0BSD, for iceoryx2's `enum-iterator` dependency; `xtask deps`
  excuses iceoryx2's QNX-only `cc` and `bindgen` build helpers beneath the shared-memory,
  server and CLI crates only.

### Stability

- Segments also seal after `[storage] max_segment_open_time_ms` of wall-clock time
  open (default one hour), so a quiet stream reaches sealed segments without a stop.
- `VerifyStream` reads the chunks already written to the open segment, by header and
  CRC; `nornctl verify` prints `buffered=` and exits 1 when it examined nothing while
  the stream holds samples. `segment_count` counts sealed segments only.
- `nornctl compact` says when there is nothing sealed to compact, and a failed
  connection to a Unix socket names the usual causes.
- Shutdown: an `AppendSamples` call held open answers its admitted batches and ends
  with `UNAVAILABLE` and `ERROR_REASON_SHUTTING_DOWN`; an idle connection no longer
  delays the stop.
- Durability on macOS: `fsync` is `F_FULLFSYNC` through Rust's `std`, now documented
  with its cost.

### Known issue

- A maximum-speed replay of a recent window may stop after `RUNNING`; under
  investigation.

## 0.1.0 — 2026-11-03

First release of Norn, a pure-Rust, domain-independent temporal data engine.

### Included

- The `norn` daemon and `nornctl` client, communicating over local gRPC on
  Unix sockets or owner-only Windows named pipes.
- Protocol 1.6, including `BatchCreateEntities` and `BatchCreateSignals`:
  up to 1,000 resources per call, all or none, with one durable commit.
- Stream, entity and signal metadata; timestamped samples with explicit
  backpressure, bounded buffers and buffered, persisted or durable acknowledgements.
- A write-ahead journal, lossless compression, checksummed segment storage,
  a sparse temporal index and crash recovery.
- Range queries, filters, aggregates, resume tokens, state at an instant,
  deterministic replay and live subscriptions.
- Compaction, retention and storage verification.
- CSV, Parquet, Arrow IPC and MCAP import and export through `nornctl`.
- Seventeen read-only MCP tools built into the daemon (`norn --mcp`),
  disabled by default. gRPC is always on.
- A build with no C toolchain and no TLS stack.
- Configuration and operations guides, CLI and protocol reference, and
  reproducible performance measurements.

### Distribution

- Prebuilt archives and SHA-256 checksums for macOS Apple silicon and Intel,
  Linux AArch64 and x86-64, and Windows x86-64. Linux requires glibc 2.35 or newer.
- `shokki-engineering/tap/norn` installs the complete program on macOS and Linux.
  `brew services start norn` runs the daemon in the background and at login,
  with persistent configuration, private data and socket directories, and logs.
- Release automation updates the Homebrew tap with a dedicated PAT and the
  Git author name Walle. Release builds include documentation and licences.
- All components use AGPL-3.0-or-later, with a separate commercial licence.

### Known limits

- Shared memory, fuzzing and a security review remain planned; there is no
  metrics export, backup tooling or encryption at rest.
- Device-protocol adapters are supplied separately, as clients of the public
  API, and are not bundled into Norn. Further protocol adapters remain planned.
- Rust crates are not published to crates.io; use the source repository
  at the `v0.1.0` tag for library dependencies.
