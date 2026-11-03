<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Changelog

## 0.1.0 — 2026-11-03

First release of Norn, a pure-Rust, domain-independent temporal data engine.

### Included

- The `norn` daemon and `nornctl` client, communicating over local gRPC on
  Unix sockets or owner-only Windows named pipes.
- Stream, entity and signal metadata; timestamped samples with explicit
  backpressure, bounded buffers and buffered, persisted or durable acknowledgements.
- A write-ahead journal, lossless compression, checksummed segment storage,
  a sparse temporal index and crash recovery.
- Range queries, filters, aggregates, resume tokens, state at an instant,
  deterministic replay and live subscriptions.
- Compaction, retention and storage verification.
- CSV, Parquet, Arrow IPC and MCAP import and export through `nornctl`.
- Seventeen read-only MCP tools built into the daemon, disabled by default.
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
- BACnet capture is supplied separately by bacnet-rs and is not bundled
  into Norn. Modbus, OPC-UA and MQTT adapters remain planned.
- Rust crates are not published to crates.io; use the source repository
  at the `v0.1.0` tag for library dependencies.
