<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

<div align="center">

<h1>Norn</h1>

<p><strong>A pure-Rust, domain-independent temporal data engine.</strong><br>
It stores the history of sensor data and device telemetry, answers questions about any instant of it, and replays it.</p>

<p>
<a href="https://github.com/shokki-engineering/norn/actions/workflows/ci.yml"><img alt="CI" src="https://img.shields.io/github/actions/workflow/status/shokki-engineering/norn/ci.yml?branch=main&style=flat-square&logo=githubactions&logoColor=white&label=CI"></a>
<a href="https://github.com/shokki-engineering/norn/releases"><img alt="Release" src="https://img.shields.io/github/v/release/shokki-engineering/norn?include_prereleases&style=flat-square&label=release"></a>
<a href="LICENSE"><img alt="Licence: AGPL-3.0-or-later or commercial" src="https://img.shields.io/badge/licence-AGPL--3.0--or--later%20%7C%20commercial-blue?style=flat-square"></a>
<a href="rust-toolchain.toml"><img alt="Rust 1.99 MSRV" src="https://img.shields.io/badge/Rust-1.99%20MSRV-B7410E?style=flat-square&logo=rust&logoColor=white"></a>
<a href="https://github.com/shokki-engineering/norn/commits/main"><img alt="Last commit" src="https://img.shields.io/github/last-commit/shokki-engineering/norn/main?style=flat-square&label=last%20commit"></a>
</p>

<p>
<a href="#installation"><img alt="Platforms: Linux, macOS, Windows" src="https://img.shields.io/badge/platforms-Linux%20%7C%20macOS%20%7C%20Windows-lightgrey?style=flat-square"></a>
<a href="https://grpc.io"><img alt="gRPC: always on" src="https://img.shields.io/badge/gRPC-always%20on-244C5A?style=flat-square"></a>
<a href="protobuf"><img alt="Protocol crate 1.0.0-rc.2" src="https://img.shields.io/badge/protocol-1.0.0--rc.2-244C5A?style=flat-square"></a>
<a href="https://modelcontextprotocol.io"><img alt="MCP: optional, built in" src="https://img.shields.io/badge/MCP-optional%2C%20built%20in-244C5A?style=flat-square&logo=modelcontextprotocol&logoColor=white"></a>
<a href="docs/reference/architecture.md#no-c"><img alt="No C, no TLS" src="https://img.shields.io/badge/build-no%20C%2C%20no%20TLS-50ABF1?style=flat-square"></a>
</p>

<p>
<a href="https://tokio.rs"><img alt="tokio 1.54" src="https://img.shields.io/badge/tokio-1.54-555555?style=flat-square&logo=tokio&logoColor=white"></a>
<a href="https://github.com/hyperium/tonic"><img alt="tonic 0.14" src="https://img.shields.io/badge/tonic-0.14-555555?style=flat-square"></a>
<a href="buf.yaml"><img alt="buf 1.75" src="https://img.shields.io/badge/buf-1.75-244C5A?style=flat-square"></a>
<a href="https://parquet.apache.org"><img alt="Apache Parquet 60" src="https://img.shields.io/badge/Parquet-60-50ABF1?style=flat-square&logo=apacheparquet&logoColor=white"></a>
<a href="https://arrow.apache.org"><img alt="Apache Arrow 60" src="https://img.shields.io/badge/Apache%20Arrow-60-50ABF1?style=flat-square&logo=apachearrow&logoColor=white"></a>
<a href="https://mcap.dev"><img alt="MCAP 0.26" src="https://img.shields.io/badge/MCAP-0.26-50ABF1?style=flat-square"></a>
</p>

</div>

Norn is a local daemon. Programs send it timestamped samples over gRPC on a Unix
socket or a Windows named pipe. It journals them, compresses them losslessly into
checksummed chunks, indexes them by time, and answers range queries, aggregates, the
state of every signal at an instant, and deterministic replays. It knows entities,
signals and streams, and never what they mean.

**Release: `norn v1.0.0-rc.2`**, the second release candidate for 1.0. The daemon,
`nornctl`, the interchange adapters, the MCP server and the protocol crate are
versioned together at `1.0.0-rc.2`, and the wire protocol negotiates version 1.8. This
candidate adds backup, restore and migration, a compatibility policy, fuzzing, the
benchmark matrix, the `norn-sink` device adapter kit, the security review's fixes and
opt-in, **experimental** encryption at rest. See the
[changelog](docs/project/changelog.md) and [known issues](#status-and-known-issues).

## Contents

**This page**: [what it is and why](#what-it-is-and-why), [features](#features),
[installation](#installation), [quick start](#quick-start),
[status and known issues](#status-and-known-issues), [security](#security),
[contributing](#contributing), [licence](#licence).

**Guides**

- [Getting started](docs/guides/getting-started.md): build, run, create, append, list, and a longer walk-through
- [How Norn works](docs/guides/concepts.md): the ideas, one at a time, with screenshots
- [Using Norn from your own code](docs/guides/clients.md): gRPC, `nornctl` and MCP as clients
- [Homebrew](docs/guides/homebrew.md): installation, the background service, upgrades, release automation
- [Configuration](docs/guides/configuration.md): every key and its default
- [Ingesting data](docs/guides/ingesting-data.md): batches, acknowledgements, backpressure
- [Operations](docs/guides/operations.md): starting, stopping, the data directory, recovery, damage, verify, compaction, retention, backup, migration
- [Troubleshooting](docs/guides/troubleshooting.md): real errors and what they mean
- [Querying](docs/guides/querying.md): ranges, filters, aggregates, resuming
- [Replay and state](docs/guides/replay.md): state at an instant, replay, live subscriptions
- [Import and export](docs/guides/import-export.md): CSV, Parquet, Arrow, MCAP, both ways
- [AI agents](docs/guides/ai-agents.md): `norn --mcp`, a client configuration
- [Writing a device adapter](docs/guides/device-adapter.md): `norn-sink`, step by step, with a complete example

**Reference**

- [Architecture](docs/reference/architecture.md): the system at a glance, write and read paths, layers, dependency table, process model
- [Protocol](docs/reference/protocol.md): the gRPC API, errors, versioning
- [Storage](docs/reference/storage.md): file format, encryption, write path, recovery, index, codecs
- [Compatibility](docs/reference/compatibility.md): on-disk format versions, the protocol, the support window, what 1.x promises
- [IPC](docs/reference/ipc.md): transports, endpoints, security model
- [Shared memory](docs/reference/shared-memory.md): the opt-in iceoryx2 data plane, settings, limits, trust boundary and crash handling
- [Query](docs/reference/query.md): read path, StateAt, exact semantics, bounds
- [Replay](docs/reference/replay.md): virtual clock, sessions, subscriptions, bounds
- [Adapters](docs/reference/adapters.md): the contract, CSV, Parquet, Arrow, MCAP formats
- [nornctl](docs/reference/cli.md): every command
- [MCP](docs/reference/mcp.md): the daemon's 17 read-only tools and transports

**Security**

- [Security review, November 2026](docs/security/review-2026-11.md): threat model, findings, fixes, what was not examined

**Project**

- [Changelog](docs/project/changelog.md): every release, v0.1.0 to v1.0.0-rc.2
- [Performance](docs/performance.md): the benchmark matrix, every measurement and how it was made
- [Specification](docs/project/spec.md): the binding brief
- [Plan](docs/project/plan.md): how the work is split, its status and the [roadmap](docs/project/plan.md#roadmap)

**Images**, captured from a real daemon by
[`tools/screenshots/capture.sh`](tools/screenshots/capture.sh):
[status](docs/images/nornctl-status.svg), [version](docs/images/nornctl-version.svg),
[signals](docs/images/nornctl-signals.svg), [append](docs/images/nornctl-append.svg),
[query](docs/images/nornctl-query.svg), [streams](docs/images/nornctl-streams.svg).

## What it is and why

Many systems need the same thing from their sensor data: keep years of it on one
machine, lose nothing that was acknowledged, and answer "what did the system look
like at 08:00:15, and what happened around it?" quickly. Norn does that one job, for
any domain.

- **One process, local.** A daemon owns a data directory. Clients reach it through
  local IPC. There is no external database, and no network listener unless you turn
  one on ([security](#security)).
- **Domain-independent.** The core knows entities, signals, streams, samples, values,
  timestamps, sequences and quality. Device telemetry is the first workload, but no
  device, protocol or industry term appears in the engine. Domain facts travel as
  opaque annotations.
- **Time first.** Storage, indexing, queries, state at an instant and replay are all
  organised around time.
- **Pure Rust, no TLS.** No C in any shipped dependency graph, and no C toolchain to
  build it (`xtask deps` enforces both). The daemon serves local IPC only, so no TLS
  stack is linked. `unsafe` is denied workspace-wide, with one exception: the
  owner-only security descriptor for Windows named pipes.
- **Bounded.** Every memory budget and count limit is configured and enforced. When a
  budget runs out, clients are slowed or refused, and nothing is dropped silently.

One process: clients reach the gRPC API over local IPC, MCP calls the same API in
process, and nothing reaches storage another way while the daemon runs. The write
path, read path, adapters and storage hierarchy each have a diagram in
[architecture](docs/reference/architecture.md#the-system-at-a-glance), and the ideas
behind them are in [how Norn works](docs/guides/concepts.md).

## Features

- **Append over gRPC, with backpressure and acknowledgement levels**: Done. `ACCEPT`, `BUFFER`, `THROTTLE`, `REJECT`; `BUFFERED` to `DURABLE`
- **Batch creates**: Done. `BatchCreateEntities` and `BatchCreateSignals`, up to 1,000 a call, all or none, one durable commit
- **Journal, group commit, fsync**: Done. `DURABLE` only after `fsync` returns
- **Compressed, checksummed chunks in segment files**: Done. Delta-of-delta, Gorilla, ALP, dictionaries, zstd levels 3 to 6
- **Sparse temporal index**: Done. Rebuilt from the segments if lost
- **Range queries, filters, aggregates, resume tokens**: Done. Sees samples as soon as they are acknowledged
- **State at an instant (`StateAt`)**: Done. From the index; checkpoints measured and not needed
- **Deterministic replay and live subscriptions**: Done. Virtual clock: pause, seek, step, speed. One [known issue](#status-and-known-issues)
- **Crash recovery**: Done. Tested by killing the process, not by cutting power
- **Compaction, retention, verification**: Done. In the background and on demand ([operations](docs/guides/operations.md))
- **Backup, restore, migration**: Done, in 1.0.0-rc.2. `nornctl backup` while the daemon runs; `restore` and `migrate` offline ([operations](docs/guides/operations.md#backup-and-restore))
- **Compatibility policy**: Done, in 1.0.0-rc.2. Versioned store and segment formats, golden stores read on every CI run, and what 1.x promises ([compatibility](docs/reference/compatibility.md))
- **Import and export: CSV, Parquet, Arrow IPC, MCAP**: Done. Through `nornctl`, over the public API
- **`nornctl` command-line client**: Done, including `inspect`, `stats` and `benchmark`
- **MCP for AI agents, served by the daemon**: Done. `norn --mcp`, 17 read-only tools; off by default ([AI agents](docs/guides/ai-agents.md))
- **Device adapter kit, `norn-sink`**: Done, in 1.0.0-rc.2. Bounded buffer, retries, catalogue at start ([writing a device adapter](docs/guides/device-adapter.md))
- **Homebrew package and background service**: Done. `brew services start norn`
- **Shared-memory data plane**: Done, from 1.0.0-rc.1. Opt-in, Unix sockets only, transparent socket fallback ([shared memory](docs/reference/shared-memory.md))
- **Fuzzing**: Done. 26 cargo-fuzz targets, one per decoder of untrusted bytes, run nightly ([SECURITY.md](SECURITY.md#fuzzing))
- **Security review**: Done, one source review, not an external audit; two medium findings kept open by decision ([review](docs/security/review-2026-11.md))
- **Encryption at rest**: **Experimental** in 1.0. Opt-in, no independent review, and its on-disk form may change within 1.x. Covers chunk data and journals, not names or time ranges ([storage](docs/reference/storage.md#encryption))
- **Benchmark matrix**: Built. A quick profile runs in CI; the full run has not been made ([performance](docs/performance.md#benchmark-matrix))
- **Pattern detection, GGML-accelerated**: Planned. Opt-in, outside the engine; scope still to be decided
- **Further device-protocol adapters**: Planned. As clients of the public API, outside this repository
- **Metrics export**: Not built

## Installation

**Homebrew**, on macOS and Linux. The tap and archives are public; no GitHub token is
needed.

```sh
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The service runs at login, restarts after a crash, keeps data in
`$(brew --prefix)/var/norn` and reads `$(brew --prefix)/etc/norn/norn.toml`. After
`brew upgrade norn`, run `brew services restart norn`. Details:
[Homebrew](docs/guides/homebrew.md).

**From source**, with Rust 1.99.1 (pinned in `rust-toolchain.toml`) and `protoc`, and
no C toolchain:

```sh
git clone https://github.com/shokki-engineering/norn.git
cd norn
git checkout v1.0.0-rc.2
cargo build --release --locked -p norn-server -p norn-cli
```

This builds `target/release/norn`, the daemon, and `target/release/nornctl`, the
client. **Release archives** for Linux, macOS and Windows, each with a `.sha256` file,
are on the [releases page](https://github.com/shokki-engineering/norn/releases).
Prerequisites and the platform list: [getting started](docs/guides/getting-started.md#build).

## Quick start

Start the daemon in one terminal (`./target/release/norn`, or use the Homebrew
service and add its `--endpoint` to each call). In another, create a catalogue,
append, and read it back. This output was captured from Norn 0.1.0; the commands are
unchanged.

```console
$ nornctl streams create plant
streams/plant
$ nornctl entities create dev-01
entities/dev-01
$ nornctl signals create dev-01 temperature plant double
entities/dev-01/signals/temperature
$ nornctl append plant entities/dev-01/signals/temperature 21.0 --at 2026-11-02T08:00:00Z --sequence 1
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
$ nornctl state dev-01 --at 2026-11-02T08:00:15Z
state_time: 2026-11-02T08:00:15Z
2026-11-02T08:00:00Z entities/dev-01/signals/temperature 21 GOOD seq=1
```

Each batch hears its admission, then the level it reached. The full walk-through, with
queries, aggregates, replay, verify, compaction, export and import, is in
[getting started](docs/guides/getting-started.md#a-longer-walk-through).

## Status and known issues

1.0.0-rc.2 is a release candidate: the owner field-tests it before tagging 1.0.0.
What is built, measured and planned, stage by stage, is in the
[roadmap](docs/project/plan.md#roadmap).

- **Replay.** A maximum-speed replay of a recent window may stop after `RUNNING`;
  under investigation.
- **Encryption at rest is experimental.** It has had no independent review, and its
  on-disk form may change within 1.x, so it is outside the
  [compatibility promise](docs/reference/compatibility.md#what-1x-promises). A backup
  of an encrypted store that holds no segment yet loses its encryption marker, so
  restoring it without a key opens it as plain
  ([operations](docs/guides/operations.md#backup-and-restore)).
- **Crash tests cover process death** (`kill -9`), not power loss. Every performance
  figure is from macOS; Linux and Windows are tested in CI, not measured.
- **No disk budget.** A client can write until the disk is full unless a retention
  policy is set ([review](docs/security/review-2026-11.md), L1).

## Security

Report vulnerabilities privately, through GitHub's private vulnerability reporting:
see [SECURITY.md](SECURITY.md). Every decoder of untrusted bytes is fuzzed nightly. One
source review has been done ([docs/security/review-2026-11.md](docs/security/review-2026-11.md)),
with findings still open.

- **Listeners.** By default the daemon serves only an owner-only Unix socket or named
  pipe. `listen = "tcp:..."` and MCP over HTTP have no authentication: once turned on,
  every local user can reach them, and the daemon only warns at start. Use the Unix
  socket, a named pipe or MCP over stdio instead.
- **Encryption at rest** is opt-in and experimental. It covers sample data, not names
  or time ranges ([storage](docs/reference/storage.md#encryption)).

## Contributing

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. In short: no source file reaches 200
lines, no C, no new `unsafe`, and `just ci` must pass, including the repository gates.
Every gate fails when it examined nothing, and each has a test proving it bites.

```sh
just test    # every test, including the tests proving each gate bites
just lint    # cargo fmt --check and clippy -D warnings
just gates   # size, purity, docs, arch, deps
just ci      # everything CI runs
```

`xtask docs` also checks this README: every relative link and anchor resolves, the
Contents list every file under `docs/` and nothing else, and the README holds no
emoji or other pictograph.

## Licence

Copyright © 2026 Srikanth K. All rights reserved, except as granted below.

Norn is dual-licensed:

- **[AGPL-3.0-or-later](LICENSE)** for open-source use.
- **[Commercial](LICENSE-COMMERCIAL.md)** for proprietary use under a separate agreement.

This covers every part of Norn, with no exception: the engine, the server, the
client and protocol crates, their generated code, and the `.proto` definitions
under [`protobuf/`](protobuf). A proprietary client built on any of them needs
a commercial licence.

Contributions follow the [Contributor Licence Agreement](.github/CLA.md);
sign off each commit with `git commit -s`. Third-party dependencies retain their own
licences, which `deny.toml` keeps permissive. SPDX headers, the generated MCP
handlers' terms and the licence files in release archives:
[CONTRIBUTING.md](CONTRIBUTING.md#licence-and-contributions).
