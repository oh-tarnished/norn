<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Getting started

Everything here works today. It builds Norn, starts the daemon, creates a
stream, appends a sample, and reads it back.

All output below was captured from a real `norn` 0.1.0 on macOS; CI tests these
commands on Linux and Windows too.

## Build

Install Rust 1.99.1, pinned in `rust-toolchain.toml`, and `protoc` (CI uses 36.2).
Keep `protoc` on `PATH`, or set `PROTOC` to its executable. Norn's generated code
is committed, but runtime-rs's validation dependency generates its types during
the build. No C compiler is needed: runtime-rs is built without TLS, so nothing
in the graph links C.

```sh
cargo build -p norn-server -p norn-cli
```

This gives you two binaries in `target/debug/`: `norn`, the daemon, which also
serves AI agents over MCP when asked ([AI agents](ai-agents.md)); and `nornctl`, a
client that uses only the public gRPC API. The commands below assume
that directory is on your `PATH`.

Or download a release archive for your platform from GitHub Releases and check it
against its `.sha256` file. Each one carries `norn` and `nornctl`
(`.exe` on Windows) with this documentation; Linux builds need glibc 2.35 or newer.

## Start the daemon

`norn` runs in the foreground and logs to stderr, so give it its own terminal.
With no flags it uses the per-user endpoint and data directory
([configuration](configuration.md)).

```console
$ norn
2026-11-01T21:19:53.375710Z  INFO registry open records=0 torn=0
2026-11-01T21:19:53.376380Z  INFO listening endpoint=unix:/tmp/norn-demo/t/norn/norn.sock version="0.1.0"
```

(One long `recovery finished` line is left out between those two, and that run
set `TMPDIR=/tmp/norn-demo/t`, which is where the default endpoint lives.)

## Check it

```console
$ nornctl status
health: SERVING
server_version: 0.1.0
protocol: 1.4
capabilities: CAPABILITY_APPEND_SAMPLES CAPABILITY_ACK_PERSISTED CAPABILITY_QUERY_RANGE CAPABILITY_QUERY_AGGREGATES CAPABILITY_STATE_AT CAPABILITY_LIVE_SUBSCRIPTION CAPABILITY_REPLAY CAPABILITY_COMPACTION CAPABILITY_RETENTION CAPABILITY_VERIFY CAPABILITY_ACK_DURABLE
start_time: 1793625130.214140000
limits.max_batch_samples: 10000
limits.max_batch_bytes: 4194304
limits.max_pending_batches: 256
limits.max_streams: 1024
limits.max_replay_sessions: 16
limits.max_subscriptions: 256
recovery.streams: 0
recovery.events: 0
recovery.corruptions: 0
recovery.replayed_samples: 0
recovery.clean_streams: 0
recovery.verified_segments: 0
recovery.vouched_segments: 0
compaction.runs: 0
compaction.segments: 0
compaction.saved_bytes: 0
compaction.failures: 0
retention.segments: 0
retention.samples: 0
retention.bytes: 0

$ nornctl version
client: nornctl/0.1.0
client_protocol: 1.4
server_version: 0.1.0
agreed_protocol: 1.4
```

## Create a stream, an entity and a signal

A stream is a sequence of observations. An entity is a thing that exists. A
signal is something measurable about an entity, recorded into one stream, with
one value type.

```console
$ nornctl streams create plant
streams/plant
$ nornctl entities create ahu-01
entities/ahu-01
$ nornctl signals create ahu-01 supply-temp plant double
entities/ahu-01/signals/supply-temp
$ nornctl signals
entities/ahu-01/signals/supply-temp VALUE_TYPE_DOUBLE streams/plant
```

## Append

`append` sends one sample, timed now, and prints each acknowledgement. It waits
for `DURABLE` unless told otherwise.

```console
$ nornctl append plant entities/ahu-01/signals/supply-temp 21.5
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
```

## List

```console
$ nornctl streams
streams/plant samples=3 buffered=3
$ nornctl query entities/ahu-01/signals/supply-temp
2026-11-02T01:52:07.756098Z entities/ahu-01/signals/supply-temp 21.5 GOOD seq=0
2026-11-02T01:52:07.802987Z entities/ahu-01/signals/supply-temp 21.75 GOOD seq=2
2026-11-02T01:52:07.807982Z entities/ahu-01/signals/supply-temp 22 GOOD seq=3
$ sleep 61
$ nornctl streams
streams/plant samples=3
```

A sample is queryable once acknowledged at `PERSISTED`. `buffered` counts those not in
a chunk yet: they join one within `max_chunk_age_ms`, a minute by default, or sooner
under load, and nothing else changes. More in [querying](querying.md).

## Stop

Ctrl-C or SIGTERM. In-flight calls drain and the exit status is 0.

## Next

- [Ingesting data](ingesting-data.md): batches, acknowledgement levels, backpressure.
- [Configuration](configuration.md): every key and its default.
- [Operations](operations.md): restarts, crashes, damage.
- [nornctl reference](../reference/cli.md).
