<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Norn — delivery plan

How the work in [spec.md](spec.md) is split across parallel agents. Read
[spec.md](spec.md) first; this file only says who does what, in which order.

**Starts after bacnet-rs is finished.** Until then nothing here is begun.

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
| 4 | adapters | 13 | `adapters/{bacnet,mcap,parquet,csv}` (BACnet through bacnet-rs) |
| 4 | MCP + CLI | 14, 33 | MCP in `crates/server`, `crates/cli` (`nornctl`) |
| 5 | performance | 15, 35 | `benchmarks/`: 16,000 signals × 1 Hz, bursts, the 504-billion-sample/year extrapolation |
| 5 | hardening | 16, 36 | fuzzing, crash and corruption tests, Windows validation, security review |

## Connecting bacnet-rs (owner's request, 2026-11-02)

bacnet-rs is a client of the Norn daemon over local IPC, configured in
`server.toml`, and stores nothing itself in this mode. No `--capture-norn`
one-off flag: capture becomes a `[capture]` section of the server's
configuration, its existing flags override it, and `sink = "norn"` selects a
`Sink` implementation in bacnet-rs that dials `norn_ipc::connect(endpoint)` and
calls `TelemetryService.AppendSamples`. Under SPEC section 47 the adapter uses
only Norn's public client crates. Backpressure is honoured through a bounded
buffer; nothing is dropped silently.

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
| bacnet-rs | sink over IPC built and verified end to end, uncommitted in the bacnet-rs-norn worktree (feat/norn-sink) for the owner's review |
| 4 | merged: adapters (CSV, Parquet, Arrow, MCAP; `nornctl export`/`import`), built-in MCP, and operations (compaction, retention, verification). Shared memory (stage 12) is out of v0.1 |
| release | v0.1.0 first release: platform archives, checksums, complete Homebrew package and daemon service; PAT-backed tap updates authored by Walle. See [changelog](changelog.md) and [release setup](../guides/homebrew.md) |
