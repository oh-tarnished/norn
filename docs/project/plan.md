<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Norn — delivery plan

How the work in [spec.md](spec.md) is split across parallel agents. Read
[spec.md](spec.md) first; this file only says who does what, in which order.

## Rules for every agent

- [spec.md](spec.md) is binding: pure Rust core, every source file under 200 lines,
  Rustdoc on every public item, bounded memory, no domain knowledge in the
  core, platform code behind traits.
- One agent, one git worktree, one branch (`wave-N/<piece>`), created with
  `git worktree add` from `main`. Agents never edit another piece's crate;
  a needed change to a shared crate (`core`, `protocol`) goes back to the
  coordinator.
- Update the relevant design doc ([storage.md](../reference/storage.md), [ipc.md](../reference/ipc.md), …) before the code.
- Done means `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test`,
  `cargo doc` pass for the piece, and `buf lint` where it touches `protobuf/`.
- No commits to `main` by agents; the coordinator merges a wave once every
  piece in it is green, then the next wave starts.
- After a usage-limit stop, resume the same agent; never start a duplicate.

## Waves

Each wave depends only on the waves before it. Pieces inside a wave run in
parallel.

| Wave | Piece | Spec stages | Owns |
| --- | --- | --- | --- |
| 0 | foundation | 1 | workspace, every crate's empty `lib.rs` with its layer contract, `core` types (Entity, Signal, Stream, Sample, Value, Timestamp, Sequence, Quality), error model, config, root docs skeletons, CI (Linux/macOS/Windows), gates: 200-line limit, no C in core, dependency direction |
| 1 | protocol | 2 | `protobuf/` packages `norn.*`, buf lint/breaking, codegen, versioning, capability negotiation |
| 1 | transport | 3 | `transport`, `ipc`: Unix sockets, Windows named pipes, framing, connection management |
| 1 | codec | 6 | `codec`: timestamp (regular, delta, delta-of-delta), integer, boolean, enum, Gorilla floats, ALP evaluation, pure-Rust zstd at levels 3–6 (SPEC §14 decision: confirm the encoder implements them, benchmark ratio and speed per level on realistic data), property tests |
| 1 | storage | 5 | `storage`: file format (magic, versions), segment/chunk/block, checksums, recovery markers, incomplete-segment detection, corruption reporting |
| 2 | daemon + write path | 4, 8 | `runtime`, `server`: lifecycle, health, append/batch, bounded buffers, ACCEPT/BUFFER/THROTTLE/REJECT, RECEIVED…DURABLE |
| 2 | index | 7 | `index`: sparse temporal index, rebuild |
| 2 | query | 9 | `query`: range, latest, aggregates, streaming results, chunk skipping |
| 3 | state | 10 | `state`: checkpoints, StateAt, interval benchmarks |
| 3 | replay | 11 | `replay`: virtual clock, pause/seek/step/speed, live/replay unification |
| 3 | shared memory | 12 | `shared-memory`: ring buffer, fallback |
| 4 | adapters | 13 | `adapters/{contract,mcap,parquet,csv}` (device protocols live outside this repository) |
| 4 | MCP + CLI | 14, 33 | MCP in `crates/server`, `crates/cli` (`nornctl`) |
| 5 | performance | 15, 35 | `benchmarks/`: 16,000 signals × 1 Hz, bursts, the 504-billion-sample/year extrapolation |
| 5 | hardening | 16, 36 | fuzzing (`fuzz/`, `just fuzz`, nightly workflow: every decoder of untrusted bytes), crash and corruption tests, Windows validation, security review |

## External adapters (owner's request, 2026-11-02)

A device-protocol adapter is a client of the Norn daemon over local IPC and
lives in its own repository, beside the protocol library it uses. Capture is
part of that program's configuration rather than a one-off flag: a sink
implementation dials `norn_ipc::connect(endpoint)` and calls
`TelemetryService.AppendSamples`. Under SPEC section 47 the adapter uses
only Norn's public client crates. Backpressure is honoured through a bounded
buffer; nothing is dropped silently. `norn-sink` is that sink, extracted so that a
new protocol writes only its codec and a mapping (docs/guides/device-adapter.md).

## Pattern detection (owner's decision, 2026-11-02; after v0.1)

GPU-accelerated pattern detection through GGML, which is C, is an accepted
exception to SPEC section 1, on these terms: an opt-in feature, off by
default; never in the core, engine crates or the default build; a separate
program that reads through the public API, like the MCP server. Findings are
written back as signals. Without the feature or a GPU, a CPU path still works.
The deps gate allows it narrowly, with the reason beside the entry. Which
patterns to detect is still to be decided.

## Documentation restructure (owner's request, 2026-11-02)

Runs once the write path and index have merged, before query starts, so no
agent is editing the files being moved. Norn only.

- One README: the root `README.md`. Every other README's content moves into
  a guide (`crates/server/README.md` becomes `docs/guides/configuration.md`).
- `CONTRIBUTING.md` and `SECURITY.md` stay at the root. Everything else moves:
  `docs/guides/` (getting-started, configuration, ingesting-data, querying,
  replay, operations, troubleshooting), `docs/reference/` (architecture,
  protocol, storage, ipc, query, replay, adapters, mcp, cli),
  `docs/performance.md`, `docs/project/` (spec, plan), `docs/images/`.
- README order: name, one-line pitch, badges; table of contents mirroring
  `docs/`; what it is, with one Mermaid diagram; quick start with real output;
  how it works, snippets and screenshots linking to their guides; measured
  numbers, with how they were measured; roadmap table (one row per stage,
  done / in progress / planned), the only roadmap, so `ROADMAP.md` goes;
  contributing, security, licence.
- Screenshots come from the real tool, never drawn or mocked up.
- A new `xtask docs` check: relative links and images resolve; every `docs/`
  file is in the README's contents and the reverse; only the root has a README.
- SPEC section 3 and section 38 are updated to this layout, and every agent
  prompt uses the new paths.

## Status

| Wave | State |
| --- | --- |
| 0 | done: foundation merged (c22efde) |
| 1 | done: transport, protocol, codec merged |
| 2 | done: daemon, storage, index, write path and metadata merged |
| 3 | done: documentation restructure, query, StateAt and replay merged (checkpoints deferred by measurement) |
| external adapters | the first device-protocol sink over IPC is built and verified end to end in its own repository |
| 4 | merged: adapters (CSV, Parquet, Arrow, MCAP; `nornctl export`/`import`), built-in MCP, and operations (compaction, retention, verification). Shared memory (stage 12) is out of v0.1 |
| release | v0.1.0 first release: platform archives, checksums, complete Homebrew package and daemon service; PAT-backed tap updates authored by Walle. See [changelog](changelog.md) and [release setup](../guides/homebrew.md) |
| 1.0.0-rc.1 | released 2026-11-03: stage 12, the shared-memory data plane (`norn-shared-memory` over iceoryx2, protocol 1.7, opt-in, Unix only, socket fallback); stability (verify reads the open segment, sealing by open time, shutdown drain of open append calls, macOS `F_FULLFSYNC` documented). See [changelog](changelog.md) |
| 1.0.0-rc.2 | protocol 1.8: published limits, segment inspection, `nornctl inspect`/`stats`/`benchmark`, end-to-end and crash CI jobs, fuzzing, the benchmark matrix, `norn-sink`, compatibility policy, backup, restore and migrate, security review fixes, experimental encryption at rest, Windows fixes. See [changelog](changelog.md) |
| next | the open replay issue (a maximum-speed replay of a recent window may stop after `RUNNING`; under investigation), then GGML-accelerated pattern detection (opt-in), then the full benchmark matrix |

## Roadmap

**v0.1.0 and 1.0.0-rc.1 are released.** **1.0.0-rc.2 (protocol 1.8)** is the second
release candidate: published limits, `nornctl inspect`, `stats` and `benchmark`,
fuzzing, the benchmark matrix, `norn-sink`, the compatibility policy with backup,
restore and migrate, the security review's fixes and opt-in, experimental encryption
at rest ([changelog](changelog.md)). What comes after, in order:

- **The open replay issue.** A maximum-speed replay of a recent window may stop after
  `RUNNING`; under investigation.
- **Pattern detection.** GGML-accelerated and opt-in, as a separate program reading
  through the public API, never in the engine or the default build. Its scope will be
  discussed later.
- **The benchmark matrix's full run.** The matrix is built and a quick profile runs in
  CI; the specification's full range of signal counts and rates has not been measured,
  on macOS, Linux or Windows ([performance](../performance.md#benchmark-matrix)).

By stage, one row per stage of [the specification](spec.md#48-implementation-order).
`done` means built and tested: CI builds, lints and tests every push on Linux, macOS
and Windows, and cross-compiles ARMv7 and AArch64 Linux.
[The 0.1.0 validation run is green](https://github.com/shokki-engineering/norn/actions/runs/41585788519).
Tests that need Unix signals or Unix sockets run on Linux and macOS only;
[operations](../guides/operations.md#platforms) lists them.

| Stage | Subject       | Scope                                                       | State       | Today                                                                                                                                                                      |
| ----- | ------------- | ----------------------------------------------------------- | ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| 1     | Foundation    | workspace, docs, core types, errors, config, gates          | done        |                                                                                                                                                                            |
| 2 | Protocol | Protobuf, gRPC, buf, AIP lint, versioning | done | 1.6, with `BatchCreateEntities` and `BatchCreateSignals`; 1.7 in 1.0.0-rc.1 adds `SharedMemoryPlane`; 1.8 in 1.0.0-rc.2 adds the published message limits, `GetSegment`, `ListSegments` and `CreateBackup`; `ExportStream` unimplemented by decision: export is client-side |
| 3     | IPC           | Unix sockets, named pipes, transport, framing               | done        | named pipes run in CI on `windows-latest`: `nornctl` against a real daemon, and the owner-only DACL                                                                        |
| 4     | Basic daemon  | lifecycle, config, health                                   | done        | Homebrew service; in 1.0.0-rc.1 an open append call or idle connection no longer delays shutdown                                                                                |
| 5     | Storage       | format, chunks, segments, checksums, recovery               | done        | compaction, retention, `verify` and a clean-start marker run in the daemon; 1.0.0-rc.1 adds sealing by open time and verifying the open segment                                 |
| 6     | Encoding      | timestamp, integer, float, boolean, enum, zstd              | done        |                                                                                                                                                                            |
| 7     | Index         | sparse temporal index, rebuild                              | done        | a per-signal summary answers for silent signals                                                                                                                            |
| 8     | Write path    | append, batch, backpressure, durability                     | done        | live subscribers fed from the commit thread, never waited for                                                                                                              |
| 9     | Query         | range, latest, aggregates                                   | done        | sees buffered samples; measured at 200 signals, not 16,000                                                                                                                 |
| 10    | State         | checkpoints, `StateAt`                                      | done        | `StateAt` from the index; checkpoints measured and deferred                                                                                                                |
| 11    | Replay        | virtual clock, pause, seek, speed                           | done        | sessions in memory, ended by a restart                                                                                                                                     |
| 12    | Shared memory | ring buffer, fallback                                       | done        | in 1.0.0-rc.1, protocol 1.7: `norn-shared-memory` over iceoryx2, opt-in, Unix sockets only, transparent socket fallback                                                          |
| 13 | Adapters | device protocols, MCAP, Parquet, others | done | CSV, Parquet, Arrow IPC and MCAP, export and import through `nornctl`; device-protocol adapters are separate programs over IPC, built on `norn-sink` from 1.0.0-rc.2 |
| 14    | MCP           | MCP server and the rest of `nornctl`                        | done        | MCP inside `norn` (`norn --mcp`): 17 read-only tools, stdio, a socket, a pipe or loopback-only HTTP; `nornctl` adds export, import, compact, verify, retention, `inspect`, `stats`, `benchmark` |
| 15 | Optimisation | measured defaults, the full benchmark matrix | in progress | write path, storage, index, query and IPC measured once; the benchmark matrix is built and its quick profile runs in CI, but its full run has not been made and the defaults are still picked, not measured |
| 16 | Hardening | fuzzing, crash, corruption, cross-platform, security review | in progress | crash and corruption tests exist (Unix only: they `kill -9`) and run as their own CI job; CI on Linux, macOS, Windows; 26 fuzz targets, one per untrusted decoder, run nightly; one source review: its medium findings fixed in 1.0.0-rc.2 except F4 and F5, kept by decision and warned of at start; its low findings mostly open ([review](../security/review-2026-11.md)) |
