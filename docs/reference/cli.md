<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# nornctl

The `nornctl` command, through the public gRPC API only. It never touches storage.
Output below is real, from `nornctl` 0.1.0 on macOS, except where a section names
the next release (1.0.0-rc.1, protocol 1.7).

```text
usage: nornctl [OPTIONS] COMMAND

options:
  --endpoint ENDPOINT  the daemon's endpoint, such as unix:/run/norn/norn.sock
                       (default: the platform's per-user endpoint, as norn's)
  --timeout SECONDS    give up on connecting or on any call after this long
                       (default 5)
  -h, --help           print this text
```

Exit status: 0 on success, 1 when a command failed, 2 for a command line it could
not parse.

## Commands

| Command                                               | Does                                             |
| ----------------------------------------------------- | ------------------------------------------------ |
| `status`                                              | health, version, capabilities, limits, recovery  |
| `version`                                             | protocol negotiation with the daemon             |
| `streams`                                             | list streams, with sample counts                 |
| `streams create ID`                                   | create a stream                                  |
| `entities`                                            | list entities                                    |
| `entities create ID`                                  | create an entity                                 |
| `signals [ENTITY]`                                    | list signals, of one entity or all               |
| `compact STREAM`                                      | merge sealed segments, and wait                  |
| `verify STREAM`                                       | read and check every chunk; exit 1 on damage, or (1.0.0-rc.1) when it examined nothing |
| `retention get STREAM`                                | show the policy                                  |
| `retention set STREAM forever\|Nd\|Nmo\|Nb`           | set the policy: time, months or bytes            |
| `retention apply STREAM [--dry-run]`                  | run the policy now, or list what would go        |
| `signals create ENTITY ID STREAM TYPE`                | create a signal                                  |
| `append STREAM SIGNAL VALUE [--ack L] [--sequence N] [--at T] [--shared-memory]` | send one sample and wait for its ack |
| `query TARGET [FLAGS]`                                | read samples or aggregates, streamed             |
| `state TARGET [FLAGS]`                                | each signal's latest sample at an instant        |
| `replay create TARGET [FLAGS]`, `replay ACTION NAME`  | a replay session and its clock                   |
| `subscribe live TARGET`, `subscribe replay NAME`      | events as they come, numbered                    |
| `export TARGET --format F --output PATH`              | samples to a new CSV, Parquet, Arrow or MCAP file |
| `import PATH [--shared-memory] [--timing]`            | a file into the daemon, safe to repeat           |

`TYPE` is `bool`, `int32`, `int64`, `uint32`, `uint64`, `float`, `double`,
`string`, `bytes` or `enum`. `--ack` takes `buffered`, `persisted` or `durable`,
the default. `SIGNAL` is a full name such as `entities/dev-01/signals/temperature`.

## status

```console
$ nornctl status
health: SERVING
server_version: 0.1.0
protocol: 1.6
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
```

`start_time` is seconds and nanoseconds since the Unix epoch. `recovery.*` describes
the last start: see [operations](../guides/operations.md#crash-recovery).

## version

```console
$ nornctl version
client: nornctl/0.1.0
client_protocol: 1.6
server_version: 0.1.0
agreed_protocol: 1.6
```

Unlike `status`, this negotiates, so an incompatible daemon fails here with its
reason.

From 1.0.0-rc.1, `client_protocol` is 1.7, and so is `agreed_protocol` against a 1.0.0-rc.1
daemon; against a 0.1.0 daemon the agreed version is 1.6, the lower minor. A 1.0.0-rc.1
daemon with shared memory enabled adds `CAPABILITY_SHARED_MEMORY` to `status`'s
capabilities.

## Connecting

From 1.0.0-rc.1, when `nornctl` cannot reach a daemon on a Unix socket, the error names
the two usual causes: `cannot reach the daemon at ENDPOINT: ... (is the daemon
running? It also refuses to start unless the socket's directory is mode 0700; its
log says which)`.

## streams, entities, signals

```console
$ nornctl streams
streams/plant samples=3
$ nornctl entities
entities/dev-01
$ nornctl signals dev-01
entities/dev-01/signals/online VALUE_TYPE_BOOL streams/plant
entities/dev-01/signals/temperature VALUE_TYPE_DOUBLE streams/plant
```

## append

`--at T` sets the sample's `observe_time`, as `query` reads `T`; without it the sample is
timed now. See [ingesting data](../guides/ingesting-data.md).

`--shared-memory` (1.0.0-rc.1, protocol 1.7) asks for the daemon's
[shared-memory](shared-memory.md) plane. When a ring is granted, `append` prints
`plane SHARED_MEMORY` before the acknowledgement lines; otherwise the sample goes by
the socket and nothing extra is printed. The answers are the same either way, so the
flag never changes what is stored.

## Not implemented

SPEC section 33 also names `inspect`, `stats` and `benchmark`. None exists yet; each gives
`unexpected argument` and exit status 2. They come with the stages that build them
([roadmap](../../README.md#roadmap)).

## streams

```console
$ nornctl streams
streams/plant samples=7 buffered=7
```

`samples` is what a query can return. `buffered`, shown when not zero, is how many of
those are not in a chunk yet. `corrupt`, shown when not zero, counts samples in chunks
that failed verification at the last start, which `samples` leaves out.

## query

```text
nornctl query TARGET [--from T] [--to T] [--where F] [--select F] [--agg FN]
                     [--window D] [--order signal|time] [--limit N] [--resume TOKEN]
```

| Flag | Takes | Default |
| --- | --- | --- |
| `TARGET` | a signal's full name, an entity, or `all` | required |
| `--from`, `--to` | half-open bounds: RFC 3339 in UTC, Unix seconds, `now`, `now-5m` | open |
| `--where` | the sample filter, such as `"value > 20.5"` | every sample |
| `--select` | a signal filter, as ListSignals takes | every signal of `TARGET` |
| `--agg` | `latest`, `first`, `last`, `count`, `min`, `max`, `average`, `sum`, `rate` | raw samples |
| `--window` | a window width: `500ms`, `10s`, `5m`, `1h`, `1d` | one window |
| `--order` | raw order: `signal` or `time` | `signal` |
| `--limit` | stop at the first response reaching this many results, and print `resume_token:`. It can print more than this: tokens have response granularity | no limit |
| `--resume` | a token from a query with otherwise equal flags | the start |

Samples print as `time signal value quality seq=N`, aggregates as `start end signal
value count=N`, with `-` for an open end. `--timeout` bounds connecting and the first
response, not the whole stream. The [querying guide](../guides/querying.md) has real
output.

## state

```text
nornctl state TARGET [--at T] [--max-age D] [--select F]
```

| Flag | Takes | Default |
| --- | --- | --- |
| `TARGET` | a signal's full name, an entity, or `all` | required |
| `--at` | the instant: RFC 3339 in UTC, Unix seconds, `now`, `now-5m`; a sample at exactly it counts | `now` |
| `--max-age` | a duration: `500ms`, `10s`, `5m`, `1h`, `1d`; an older sample is reported unknown | no limit |
| `--select` | a signal filter, as ListSignals takes | every signal of `TARGET` |

Prints `state_time:`, then one line a signal in name order: its latest sample as
`query` prints one, or `- SIGNAL unknown` when it has none. It reads every page,
each in its own call bounded by `--timeout`. The [replay and state
guide](../guides/replay.md) has real output.

## replay

```text
nornctl replay create TARGET --from T --to T [--speed X|max] [--id ID] [--select F]
nornctl replay list
nornctl replay get|pause|resume|stop|delete NAME
nornctl replay seek NAME T
nornctl replay step NAME N|D
nornctl replay speed NAME X|max
```

| Argument | Takes |
| --- | --- |
| `TARGET` | a signal's full name, an entity, or `all`, as `query` |
| `--from`, `--to` | the half-open interval, both required, as `query` reads `T` |
| `--speed` | a multiple of real time (`0`, `1`, `10`, `100`, `0.5`; a trailing `x` is allowed), or `max`. Default `1` |
| `--id` | the session's ID: lower-case letters, digits and hyphens, a letter first. Default: chosen by the daemon |
| `NAME` | an ID or `replaySessions/ID` |
| `step` | `N`, a number of sample events, or `D`, a duration such as `10s`, of virtual time |

Every command but `list` and `delete` prints the session as one line:
`NAME STATE at=CLOCK emitted=N pace=P from=START to=END`. `list` prints every session,
oldest first. A refused control exits 1 with the daemon's reason. The [replay
guide](../guides/replay.md#replay) has real output.

## subscribe

```text
nornctl subscribe live TARGET [--select F] [--limit N]
nornctl subscribe replay NAME [--resume] [--limit N]
```

Prints each event as `#N CLOCK` and then the sample as `query` prints one, `state` and a
sample for what a signal held at a seek, or a clock event (`RUNNING`, `PAUSED`, `SEEKED`,
`SPEED_CHANGED`, `ENDED`, `STOPPED`). It stops at the end of the stream, at `ENDED`, or
after `--limit` events. `--resume` resumes the session once the subscription is open, so
nothing is missed. A live subscription runs until stopped; one that falls behind exits 1
with `ERROR_REASON_SUBSCRIBER_TOO_SLOW`. `--timeout` bounds connecting and the first
response only.

## export and import

```text
nornctl export TARGET --format csv|parquet|arrow|mcap --output PATH [--from T] [--to T] [--select F]
nornctl import PATH [--format csv|parquet|arrow|mcap] [--ack LEVEL] [--batch N] [--shared-memory] [--timing]
```

| Argument | Takes | Default |
| --- | --- | --- |
| `TARGET` | a signal's full name, an entity, or `all`, as `query` | required |
| `--format` | the file format; `import` reads it from the extension | required for `export` |
| `--output` | a file that must not exist | required |
| `--from`, `--to`, `--select` | as `query` | everything |
| `--ack` | `buffered`, `persisted` or `durable`, which each batch must reach | `durable` |
| `--batch` | samples per batch, at most the daemon's `max_batch_samples` | that limit |
| `--shared-memory` | (1.0.0-rc.1) send batches through a shared-memory ring when the daemon grants one, the socket otherwise | the socket |
| `--timing` | (1.0.0-rc.1) also print the elapsed time and rate | off; on with `--shared-memory` |

`export` prints `exported samples=N signals=N bytes=N to PATH`, and removes its file
on failure. `import` prints `imported read= sent= present= duplicate= created= batches=
throttled= retried=`; `present` counts samples the daemon already held, so a repeat
sends nothing. With `--timing` or `--shared-memory`, a second line follows:
`timing seconds=S samples_per_second=R`, the rate of samples sent. A damaged file exits 1 naming the row, such as `csv row 3: truncated`.
`--timeout` bounds connecting and each call. Both run in `nornctl`, through
`QuerySamples` and `AppendSamples`; the daemon's `ExportStream` is not used. The
[import and export guide](../guides/import-export.md) has real output, and
[adapters](adapters.md) has the formats.

## Long-running commands

`compact`, `verify` and `retention apply` start an operation and poll it. `--timeout`
bounds each poll, not the operation. Output:
[operations](../guides/operations.md#compaction).

In 1.0.0-rc.1:

- `verify` prints `STREAM segments=N chunks=N samples=N bytes=N damaged_chunks=N
  damaged_samples=N buffered=N`. `segments` counts sealed segments; `chunks` and
  `samples` include the chunks already written to the open segment; `buffered` counts
  samples still in the write buffers, in no file yet and not examined. It exits 1 on
  damage, and also when it examined nothing while the stream holds samples, with the
  reason, such as `examined nothing: all N samples are still in the write buffers, in
  no segment file yet; they are written within storage.max_chunk_age, so verify again
  after that`.
- `compact` with no sealed segment waiting prints `STREAM nothing to compact: no sealed
  segment is waiting. Compaction rewrites sealed segments only; the open one seals when
  it is full, when it reaches storage.max_segment_open_time, or when the daemon stops`,
  rather than `chunks=0->0`.
