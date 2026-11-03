<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Troubleshooting

Every message below came from a real run. Compaction is not implemented, so it has no
entries here.

## `cannot reach the daemon`

```console
$ nornctl status
nornctl: cannot reach the daemon at unix:/tmp/norn-demo/t/norn/norn.sock: ipc grpc: transport error
```

The daemon is not running, or `nornctl` is looking at another endpoint. The default
depends on `XDG_RUNTIME_DIR` and `TMPDIR`, so the two must see the same values, or
pass the same `--endpoint` to both. The daemon logs its endpoint on the `listening`
line.

From 1.0.0-rc.1, on a Unix socket the message adds the other usual cause: `(is the daemon
running? It also refuses to start unless the socket's directory is mode 0700; its log
says which)`. See the next entry for the directory.

## `socket directory ... must be owner-only`

```text
ERROR cannot start error=ipc transport: socket directory /tmp/norn-demo has mode 755; it must be owner-only (0700)
```

The daemon will not put its socket in a directory others can enter. Use one made
with `mkdir -m 700`, or leave `--listen` at its default, which creates its own.

## `a live server owns socket`

```text
ERROR cannot start error=ipc transport: a live server owns socket /tmp/norn-demo/norn.sock
```

A daemon is already on that endpoint. A stale socket from a crash is cleaned up
for you, so this one is alive.

## `another process holds .../LOCK`

```text
ERROR cannot start error=another process holds /tmp/norn-demo/data/metadata/LOCK
```

Another daemon owns that data directory, even if on a different endpoint.

## Exit status 2 at startup

The command line or configuration was refused, and the usage text follows.

```text
norn: bad.toml: TOML parse error at line 1, column 1
  |
1 | nope = 1
  | ^^^^
unknown field `nope`, expected one of `listen`, `data_dir`, `drain_timeout_ms`, `log`, `memory`, `limits`, `metadata`, `storage`
```

```text
norn: engine configuration: invalid config MemorySubsystems: ExceedsTotal(436207616)
```

`[memory]` subsystems must sum to at most `total_bytes`. The second line came from
setting `total_bytes = 1`.

## `buffered=` beside `samples=`

Expected. `samples` is what a query can return, and `buffered` how many of them are
not in a chunk yet, for up to `max_chunk_age_ms` (a minute by default). They are
queryable and safe in the journal. See [getting started](getting-started.md#list).

## `DATA_LOSS` from a query

A chunk failed its checksum. See [querying](querying.md#when-a-query-fails) and
[operations](operations.md#damage).

## `NotFound` or `AlreadyExists`

```console
$ nornctl append plant entities/nope/signals/x 1
nornctl: SignalService.GetSignal: NotFound: Signal nope/x not found (norn.local ERROR_REASON_RESOURCE_NOT_FOUND) resource=nope/x
$ nornctl streams create plant
nornctl: StreamService.CreateStream: AlreadyExists: Stream plant already exists (norn.local ERROR_REASON_RESOURCE_ALREADY_EXISTS) resource=plant
```

Create the signal first. A client should treat `ALREADY_EXISTS` as "a past run did
this".

## `must start with a lowercase letter`

```console
$ nornctl streams create Bad_Name
nornctl: StreamService.CreateStream: InvalidArgument: stream_id: must start with a lowercase letter (norn.local ERROR_REASON_INVALID_ARGUMENT) field=stream_id
```

## `this daemon is configured without fsync`

`durable_acks = false` is set and the request asked for `DURABLE`, the default of
`nornctl append`. Pass `--ack persisted`, or turn fsync back on.

## `VALUE: a number`

```console
$ nornctl append plant entities/dev-01/signals/temperature hot
nornctl: usage: VALUE: a number
```

The value must parse as the signal's type, which `nornctl signals` shows.

## `recovery.corruptions` is not zero

See [operations](operations.md#damage). The log's `WARN` line names the file and
offset.

## A stop waits for `drain_timeout_ms`

```text
WARN shutdown timeout: aborting tasks aborted=1
```

In 0.1.0, an `AppendSamples` call that a client held open between batches, or an
idle client connection, kept the graceful drain waiting until `drain_timeout_ms` (10 s
by default) and was then aborted. From 1.0.0-rc.1 neither delays a stop: an open append
call answers every batch it already admitted and then ends with `UNAVAILABLE` and
`ERROR_REASON_SHUTTING_DOWN`, and the client reconnects and resends what was not
acknowledged. The remaining cause is a client holding a `grpc.health.v1` `Health/Watch`
stream open; close it on shutdown, or lower `drain_timeout_ms`.

## `cannot resume a session that is stopped`

```console
$ nornctl replay resume morning
nornctl: ReplayService: FailedPrecondition: cannot resume a session that is stopped (norn.local ERROR_REASON_REPLAY_STATE_CONFLICT)
```

A replay control the session's state forbids. A stopped session takes nothing but
`delete`; an ended one needs a `seek` before it plays again; a step needs the session
paused (or running at speed 0). `nornctl replay get NAME` shows the state. The table of
what each state allows: [replay.md](../reference/replay.md#states-and-controls).

## A live subscription ends with `ERROR_REASON_SUBSCRIBER_TOO_SLOW`

The subscriber read more slowly than samples arrived, and its queue of 64 batches
filled. Norn ends it rather than skip events, and ingestion never waits for it. Every
event before the end was delivered, numbered without a gap: resubscribe, and query the
interval since the last `observe_time` you saw to fill the gap.

