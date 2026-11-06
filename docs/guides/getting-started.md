<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Getting started

Everything here works today. It builds Norn, starts the daemon, creates a
stream, appends a sample, and reads it back.

All output below is real, from macOS: starting the daemon and checking it from
`norn` 1.0.0-rc.2, the rest from 0.1.0, where the commands were the same. CI tests
these commands on Linux and Windows too.

## Build

| Needs | Version and why |
| --- | --- |
| Rust | 1.99.1. Pinned in `rust-toolchain.toml`; `rustup` installs it on first build |
| `protoc` | 36.2 is used in CI. The daemon's runtime-rs validation dependency compiles its own protobuf types during the build; Norn's generated code is committed |
| C toolchain | None. The build links no C library and no TLS stack |
| Operating system | Linux, macOS, Windows. Linux release builds need glibc 2.35 or newer |
| `just` | Any recent. Optional; runs the same commands CI runs |

Working on Norn itself also needs Buf 1.75.0, cargo-deny 0.20.2 and Go for the AIP
linter: see [CONTRIBUTING.md](../../CONTRIBUTING.md).

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
The [release workflow](../../.github/workflows/release.yml) builds one archive per
target:

- `x86_64-unknown-linux-gnu`: Ubuntu 22.04
- `aarch64-unknown-linux-gnu`: Ubuntu 22.04 ARM
- `aarch64-apple-darwin`: macOS 15
- `x86_64-apple-darwin`: macOS 15 Intel
- `x86_64-pc-windows-msvc`: Windows

Homebrew installs the same archives as a background service: [Homebrew](homebrew.md).

## Start the daemon

`norn` runs in the foreground and logs to stderr, so give it its own terminal.
With no flags it uses the per-user endpoint and data directory
([configuration](configuration.md)).

```console
$ norn
2026-11-06T07:46:42.940008Z  INFO registry open records=0 torn=0
2026-11-06T07:46:42.968566Z  INFO listening endpoint=unix:/tmp/norn-text.y8D3fN/run/norn/norn.sock version="1.0.0-rc.2"
```

(One long `recovery finished` line is left out between those two, and that run
set `TMPDIR` to a fresh temporary directory, which is where the default endpoint lives.)

## Check it

```console
$ nornctl status
health: SERVING
server_version: 1.0.0-rc.2
protocol: 1.8
capabilities: CAPABILITY_APPEND_SAMPLES CAPABILITY_ACK_PERSISTED CAPABILITY_QUERY_RANGE CAPABILITY_QUERY_AGGREGATES CAPABILITY_STATE_AT CAPABILITY_LIVE_SUBSCRIPTION CAPABILITY_REPLAY CAPABILITY_COMPACTION CAPABILITY_RETENTION CAPABILITY_VERIFY CAPABILITY_ACK_DURABLE
start_time: 1793951202.834031000
limits.max_batch_samples: 10000
limits.max_batch_bytes: 4194304
limits.max_message_bytes: 262144
limits.max_batch_create_requests: 1000
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
client: nornctl/1.0.0-rc.2
client_protocol: 1.8
server_version: 1.0.0-rc.2
agreed_protocol: 1.8
```

## Create a stream, an entity and a signal

A stream is a sequence of observations. An entity is a thing that exists. A
signal is something measurable about an entity, recorded into one stream, with
one value type.

```console
$ nornctl streams create plant
streams/plant
$ nornctl entities create dev-01
entities/dev-01
$ nornctl signals create dev-01 temperature plant double
entities/dev-01/signals/temperature
$ nornctl signals
entities/dev-01/signals/temperature VALUE_TYPE_DOUBLE streams/plant
```

## Append

`append` sends one sample, timed now, and prints each acknowledgement. It waits
for `DURABLE` unless told otherwise.

```console
$ nornctl append plant entities/dev-01/signals/temperature 21.5
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
```

## List

```console
$ nornctl streams
streams/plant samples=3 buffered=3
$ nornctl query entities/dev-01/signals/temperature
2026-11-02T01:52:07.756098Z entities/dev-01/signals/temperature 21.5 GOOD seq=0
2026-11-02T01:52:07.802987Z entities/dev-01/signals/temperature 21.75 GOOD seq=2
2026-11-02T01:52:07.807982Z entities/dev-01/signals/temperature 22 GOOD seq=3
$ sleep 61
$ nornctl streams
streams/plant samples=3
```

A sample is queryable once acknowledged at `PERSISTED`. `buffered` counts those not in
a chunk yet: they join one within `max_chunk_age_ms`, a minute by default, or sooner
under load, and nothing else changes. More in [querying](querying.md).

## Stop

Ctrl-C or SIGTERM. In-flight calls drain and the exit status is 0.

## A longer walk-through

Every step from creating a catalogue to exporting it, with two signals. Use two
terminals: `norn` in the first, `nornctl` in the second. Homebrew users can use their
running service by adding `--endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock"`
to `nornctl` calls. The output was captured from Norn 0.1.0 on macOS, which spoke
protocol 1.6; a current build reports its own version and protocol 1.8.

**Check the connection and create a catalogue.** `version` negotiates the protocol.

```console
$ nornctl version
client: nornctl/0.1.0
client_protocol: 1.6
server_version: 0.1.0
agreed_protocol: 1.6
$ nornctl streams create plant
streams/plant
$ nornctl entities create dev-01
entities/dev-01
$ nornctl signals create dev-01 temperature plant double
entities/dev-01/signals/temperature
$ nornctl signals create dev-01 frequency plant double
entities/dev-01/signals/frequency
```

**Append.** Each batch hears its admission, then the level it reached.

```console
$ nornctl append plant entities/dev-01/signals/temperature 21.0 --at 2026-11-02T08:00:00Z --sequence 1
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
```

Five more were appended the same way: `frequency` 40 at 08:00:00, `temperature` 21.4
at 08:00:10, `frequency` 55 at 08:00:20, and `temperature` 21.9 at 08:00:30, sent
twice with the same sequence number.

**Query.** The resend is stored once.

```console
$ nornctl query dev-01 --order time
2026-11-02T08:00:00Z entities/dev-01/signals/temperature 21 GOOD seq=1
2026-11-02T08:00:00Z entities/dev-01/signals/frequency 40 GOOD seq=2
2026-11-02T08:00:10Z entities/dev-01/signals/temperature 21.4 GOOD seq=3
2026-11-02T08:00:20Z entities/dev-01/signals/frequency 55 GOOD seq=4
2026-11-02T08:00:30Z entities/dev-01/signals/temperature 21.9 GOOD seq=5
$ nornctl query dev-01 --agg max --window 30s --from 2026-11-02T08:00:00Z --to 2026-11-02T08:01:00Z
2026-11-02T08:00:00Z 2026-11-02T08:00:30Z entities/dev-01/signals/frequency 55 count=2
2026-11-02T08:00:00Z 2026-11-02T08:00:30Z entities/dev-01/signals/temperature 21.4 count=2
2026-11-02T08:00:30Z 2026-11-02T08:01:00Z entities/dev-01/signals/temperature 21.9 count=1
```

**State at an instant.** What every signal held at 08:00:15.

```console
$ nornctl state dev-01 --at 2026-11-02T08:00:15Z
state_time: 2026-11-02T08:00:15Z
2026-11-02T08:00:00Z entities/dev-01/signals/frequency 40 GOOD seq=2
2026-11-02T08:00:10Z entities/dev-01/signals/temperature 21.4 GOOD seq=3
```

**Replay.** A session starts paused; subscribing with `--resume` starts its clock.
`--speed max` plays as fast as the subscriber reads.

```console
$ nornctl replay create dev-01 --from 2026-11-02T08:00:00Z --to 2026-11-02T08:01:00Z --speed max --id morning
replaySessions/morning PAUSED at=2026-11-02T08:00:00Z emitted=0 pace=max from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl subscribe replay morning --resume
#1 2026-11-02T08:00:00Z RUNNING
#2 2026-11-02T08:00:00Z 2026-11-02T08:00:00Z entities/dev-01/signals/temperature 21 GOOD seq=1
#3 2026-11-02T08:00:00Z 2026-11-02T08:00:00Z entities/dev-01/signals/frequency 40 GOOD seq=2
#4 2026-11-02T08:00:10Z 2026-11-02T08:00:10Z entities/dev-01/signals/temperature 21.4 GOOD seq=3
#5 2026-11-02T08:00:20Z 2026-11-02T08:00:20Z entities/dev-01/signals/frequency 55 GOOD seq=4
#6 2026-11-02T08:00:30Z 2026-11-02T08:00:30Z entities/dev-01/signals/temperature 21.9 GOOD seq=5
#7 2026-11-02T08:01:00Z ENDED
```

**Where the samples are.** They are safe in the journal and still buffered: no
chunk has been written yet, so `verify` has nothing to read. A clean stop (Ctrl-C)
stores the buffers as chunks. After restarting `norn`, one more append at 08:00:40,
and another stop and start, the stream holds two segments:

```console
$ nornctl verify plant
streams/plant segments=2 chunks=3 samples=6 bytes=633 damaged_chunks=0 damaged_samples=0
```

**Compact, then verify.** Compaction merges neighbouring segments and re-encodes
each signal's chunks. `verify` then reads and checks every chunk again.

```console
$ nornctl compact plant
streams/plant chunks=3->2 bytes=633->408 skipped_segments=0
$ nornctl verify plant
streams/plant segments=1 chunks=2 samples=6 bytes=408 damaged_chunks=0 damaged_samples=0
```

**Export.** An export never overwrites a file.

```console
$ nornctl export dev-01 --format parquet --output dev-01.parquet
exported samples=6 signals=2 bytes=4452 to dev-01.parquet
$ nornctl export dev-01 --format csv --output dev-01.csv
exported samples=6 signals=2 bytes=989 to dev-01.csv
```

The file carries the catalogue too, so importing it into an empty daemon recreates the
stream, the entity and the signals:

```console
$ nornctl import dev-01.parquet --ack persisted
imported read=6 sent=6 present=0 duplicate=0 created=4 batches=1 throttled=0 retried=0
$ nornctl query dev-01 --agg count
- - entities/dev-01/signals/frequency 2 count=2
- - entities/dev-01/signals/temperature 4 count=4
```

**Shut down.** Ctrl-C or SIGTERM. In-flight calls drain and the exit status is 0.

```text
INFO shutdown requested signal="SIGTERM"
INFO runtime draining tasks=5
INFO ipc drained aborted=0
INFO runtime stopped aborted=0
INFO stopped aborted=0
```

## Next

- [Ingesting data](ingesting-data.md): batches, acknowledgement levels, backpressure.
- [Configuration](configuration.md): every key and its default.
- [Operations](operations.md): restarts, crashes, damage.
- [nornctl reference](../reference/cli.md).
