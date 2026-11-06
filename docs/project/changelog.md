<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Changelog

## 1.0.0-rc.2

Release title: `norn v1.0.0-rc.2`. All workspace crates, including `norn-protocol`,
are versioned `1.0.0-rc.2`; wire negotiation is at protocol 1.8. Client and daemon agree
the lower minor, so a 1.8 client and a 1.7 or 1.6 daemon still agree a version.

### Protocol 1.8 and `nornctl`

- Published limits: `Limits.max_message_bytes` (256 KiB) and
  `Limits.max_batch_create_requests` (1000), the values the daemon already enforced, so a
  client sizes batches from them instead of guessing. `nornctl status` prints them. The
  batch create comments said 4 MiB; the limit in force was always 256 KiB.
- `StreamService.GetSegment` and `ListSegments`: a stream's on-disk layout, read-only.
- `nornctl inspect`, `stats` and `benchmark`.

### Backup, restore, migration, compatibility

- [Compatibility](../reference/compatibility.md): the on-disk versions of the segment, chunk,
  journal, metadata log and index snapshot, which a daemon reads and writes, the protocol's
  major and minor rules, and the support window.
- Golden stores of today's format, checked in under `crates/runtime/tests/golden/v1`, and a
  test that opens them. A change that stops them reading fails CI.
- `<database>/FORMAT` records the store version. The daemon refuses a newer one, one mid-restore
  or mid-migration, or one it must migrate first, saying what to run. A store with no `FORMAT` is
  version 1, read in place, and gets one.
- `AdminService.CreateBackup`, a long-running operation. `nornctl backup --to DIR`
  takes a consistent copy while the daemon ingests, and `--incremental-from` copies only new
  sealed segments. `nornctl restore --from DIR --data-dir D` writes one back, offline, checking
  every file. Encrypted segments are opaque bytes to both. See
  [operations](../guides/operations.md#backup-and-restore).
- `nornctl migrate --data-dir D --to VERSION`, with `--dry-run` and `--keep-old`: offline,
  crash-safe, resumable, and checked afterwards. Today its only step is the identity on store
  version 1. See [operations](../guides/operations.md#migrating-to-a-new-store-version).
- New crate `norn-database`. The one layer rule this touches: `nornctl` may import it, for the two
  commands that have no daemon to be a client of.

- [What 1.x promises](../reference/compatibility.md#what-1x-promises): protocol major 1,
  store version 1, segment format 1.0, configuration keys, `nornctl` commands and flags,
  and the `norn-sink` API; encryption at rest and benchmark output are excluded.

### Device adapters

- New crate `norn-sink`, the protocol-neutral device adapter kit: finding and reconnecting
  to the daemon, a bounded buffer with a share per signal, batch creation cut to the
  published message limit, `AppendSamples` with backpressure, acknowledgement levels and
  resend, and the daemon's drain. An adapter writes only its codec and a mapping. See
  [writing a device adapter](../guides/device-adapter.md).

### Security

- A first source review, [November 2026](../security/review-2026-11.md), and its fixes:
  the database directory is created `0700`, or tightened to it (F1); every MCP socket's
  directory is held to the gRPC socket's checks, and relative socket paths and remote pipe
  names are refused (F2); MCP over HTTP refuses a browser `Origin` that is not loopback or
  a configured host (F3); new directories are synced to their parents, so that a power
  cut should not lose a new stream's directory entries (F6, found by reading and not
  reproduced); a socket directory that is a symbolic
  link is refused (F7).
- `listen = "tcp:..."` and MCP over HTTP stay open to every local user once turned on,
  by the owner's decision, and the daemon now warns at every start when either is on
  (F4, F5). See [SECURITY.md](../../SECURITY.md).
- **Encryption at rest, experimental**: `[storage] encryption = "on"` with a key file or
  environment variable. ChaCha20-Poly1305 per chunk, journals included, as format 1.1
  segments; names and time ranges are not encrypted. `nornctl rekey` rotates keys.
  Opt-in, with no independent review, and its on-disk form may change within 1.x. See
  [storage](../reference/storage.md#encryption).
- Fuzzing: 26 cargo-fuzz targets, one for every decoder of untrusted bytes, run by
  `just fuzz` and a nightly workflow, and five fixes for what they found. Among them, a
  panic in the `arrow`, `parquet` or `mcap` crates on a hostile file is caught during an
  import and reported as damage, and the lengths those crates would reserve are checked
  before they are called.

### Performance and CI

- The benchmark matrix, `just bench-matrix --quick|--full`: SPEC's workload against a
  real daemon over the socket and over shared memory. A quick profile runs in CI on
  Linux. Its full run has not been made. See [performance](../performance.md#benchmark-matrix).
- Separate end-to-end (`just e2e`) and crash and corruption (`just crash`) CI jobs on
  Linux and macOS.

### Fixes

- A resend of late data after a restart is counted as a duplicate, and queries keep the
  first-written copy.
- Shutdown: a held `grpc.health.v1.Health/Watch` stream sends `NOT_SERVING` and
  ends `UNAVAILABLE` with `ERROR_REASON_SHUTTING_DOWN` when the drain begins,
  instead of holding the drain until its bound.
- Windows: a query read that Windows refuses while compaction replaces its segment is
  retried; `nornctl migrate` opens a file for writing before syncing it, as Windows
  requires.
- Windows builds no iceoryx2 at all: `norn-shared-memory` keeps its settings and
  sizes there, with `Ring` and `SharedMemory` stand-ins that refuse, so the daemon
  and `nornctl` compile unchanged and stay on the named pipe.

### Known issues

- A maximum-speed replay of a recent window may stop after `RUNNING`; under
  investigation.
- A backup of an encrypted store that holds no segment yet does not record that the
  store was encrypted, so restored and started without a key it opens as plain. See
  [operations](../guides/operations.md#backup-and-restore).

## 1.0.0-rc.1 — 2026-11-03

Release title: `norn v1.0.0-rc.1`. All workspace crates, including `norn-protocol`,
are versioned `1.0.0-rc.1`; wire negotiation is at protocol 1.7.

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

### Client discovery

- Standard gRPC reflection v1 and v1alpha on Unix sockets and named pipes, so
  Postman discovers methods and their imported schemas. Reflection remains off
  on TCP.

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
