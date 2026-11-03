<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Operations

Working today: starting, stopping, recovery after a crash, reporting damage,
compaction, retention and verification. Export and import work: see
[import and export](import-export.md). **Not available to an operator yet:**
metrics and backup tooling. See the [roadmap](../../README.md#roadmap).

Output below is real, from macOS. CI tests Linux and Windows too ([platforms](#platforms)).

## Starting

After a clean stop a start reads no chunk: each stream's `CLEAN` marker lists its
sealed segments and their totals, and a stream that matches is taken on that word.
After a crash, whatever the marker does not vouch for is read and checked: new
segments, compaction outputs and the journal. `nornctl status` says which happened:

```console
$ nornctl status | grep recovery.
recovery.clean_streams: 1
recovery.verified_segments: 0
recovery.vouched_segments: 3
```

A start therefore does not look for bit rot in old segments. `nornctl verify` does
([Verifying](#verifying)), so schedule it. Numbers:
[performance.md](../performance.md#startup).

## Stopping

SIGINT or SIGTERM drains in-flight calls for up to `drain_timeout_ms`, then
exits 0. A running compaction stops within one chunk, before its commit. The last
thing a clean stop does is write each stream's `CLEAN` marker, which is what makes
the next start quick ([Starting](#starting)).

```text
INFO shutdown requested signal="SIGTERM"
INFO runtime draining tasks=3
INFO ipc drained aborted=0
INFO runtime stopped aborted=0
INFO stopped aborted=0
```

## The data directory

Default: `$XDG_DATA_HOME/norn`, `~/.local/share/norn`, or `%LOCALAPPDATA%\norn`.
After appending and storing one chunk (paths listed with `find`):

```text
norn/index/0000000000000001/summary.nidx
norn/metadata/LOCK
norn/metadata/metadata.log
norn/streams/0000000000000001/0000000000000000-g0000.nseg
norn/streams/0000000000000001/CLEAN
norn/streams/0000000000000001/journal/LOCK
norn/streams/0000000000000001/LOCK
```

`metadata/` is the registry of streams, entities and signals. `streams/<id>/` holds
sealed segments, and `journal/` the samples not yet stored as chunks. `index/`
can be rebuilt from the segments, and `CLEAN` can be deleted at any time: the next
start then reads everything. One daemon owns a directory at a time; a second
one stops at the lock:

```console
$ norn --listen unix:/tmp/norn-demo/n2.sock --data-dir /tmp/norn-demo/data
2026-11-01T21:19:28.862780Z ERROR cannot start error=another process holds /tmp/norn-demo/data/metadata/LOCK
```

## Crash recovery

Nothing needs doing: recovery runs at every start. A kill during a compaction
needs nothing either: `crates/server/tests/compaction_kill.rs` kills a real process
at each phase and finds every sample exactly once. Here one `DURABLE` sample is
acknowledged, the daemon gets `kill -9`, and the next start replays it from the
journal:

```console
$ nornctl append plant entities/ahu-01/signals/supply-temp 22.0 --sequence 4
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
$ kill -9 "$daemon"
$ norn &
$ nornctl status | grep recovery
recovery.streams: 1
recovery.events: 1
recovery.corruptions: 0
recovery.replayed_samples: 1
$ nornctl streams
streams/plant samples=5
```

`crates/server/tests/crash.rs` does this repeatedly mid-ingest and checks every
`DURABLE` batch sample for sample. It kills the process; it does not cut power.

The registry recovers the same way. A `BatchCreateEntities` or `BatchCreateSignals`
call is one write to `metadata/metadata.log`, framed by a batch marker, and one fsync,
so a crash keeps the whole batch or none of it. A batch cut short is a torn tail: kept
beside the log as `metadata.log.torn-<offset>`, cut, and counted in `torn_bytes` in the
`registry open` log line. Nothing acknowledged is lost, and the client's repeat of the
batch, which is safe ([protocol](../reference/protocol.md#writing-a-client)), creates
what is missing. `crates/metadata/tests/batch_recovery.rs` cuts a batch at its marker,
in its middle and in its last record.

### Upgrading and downgrading the registry log

Protocol 1.6 daemons write the registry log as version 2, which adds the batch marker.
The first start of a 1.6 daemon on a version 1 log rewrites its 16-byte header in place
and changes nothing else. A daemon from before 1.6 then refuses the directory with
`log version 2 is too new` rather than misreading a batch, so downgrading needs a copy
of the data directory taken before the upgrade.

## Damage

A checksum failure is reported and never silently repaired. Damage is found by
whatever reads the chunk: `nornctl verify`, a query, a compaction, or a start that
reads it. Here three bytes of a sealed segment were overwritten by hand after a
clean stop. The start vouches for the file without reading it, so it reports
nothing; `verify` finds it, and the count changes at once:

```console
$ nornctl status | grep recovery
recovery.corruptions: 0
recovery.clean_streams: 1
recovery.verified_segments: 0
recovery.vouched_segments: 2
$ nornctl verify plant
streams/plant segments=2 chunks=2 samples=6 bytes=466 damaged_chunks=1 damaged_samples=3
damaged 0000000000000000-g0000.nseg offset=48 length=113 BodyChecksum { stored: 932781536, computed: 43525985 }
nornctl: streams/plant: 1 damaged chunks
$ nornctl streams
streams/plant samples=3 corrupt=3
```

And after the next restart, which now reads everything:

```console
$ nornctl status | grep recovery
recovery.corruptions: 1
recovery.clean_streams: 0
recovery.verified_segments: 2
recovery.vouched_segments: 0
```

`verify` exits 1 when it finds damage. What to know:

- The file stays in place. Finding damage removes the stream's `CLEAN` marker, so
  every later start reads the whole stream and repeats the report in
  `recovery.corruptions`, until the damage is dealt with.
- `nornctl streams` takes the chunk's samples out of `samples` and shows them as
  `corrupt=N` the moment anything finds them, without a restart. Damage nothing has
  read yet is not known, and counts as ordinary samples until something does.
- A query reaching the chunk fails with `DATA_LOSS`, naming it and its time span, and
  never skips it. [Query around it](querying.md#when-a-query-fails): everything else
  stays readable.
- Compaction leaves a segment with known damage alone and reports it as
  `skipped_segments`. Retention removes one like any other, when the policy says so.
- Nothing reads quarantined files yet. There is no repair or salvage tool.

## Verifying

```console
$ nornctl verify plant
streams/plant segments=3 chunks=3 samples=9 bytes=705 damaged_chunks=0 damaged_samples=0
```

`verify` reads every chunk of every sealed segment and changes nothing. It reads
through the operating system's page cache, so run it on a schedule that suits a read
of the whole stream; it takes its turn on the maintenance worker, one job at a time.

## Compaction

Compaction merges each signal's small chunks, picks the smallest of zstd levels 3 to
6, and replaces neighbouring sealed segments with one. Queries, `StateAt`, replay and
ingest carry on throughout, and no sample is lost or doubled.

It runs by itself, every 5 minutes by default, and on demand:

```console
$ nornctl compact plant
streams/plant chunks=2->2 bytes=470->470 skipped_segments=0
$ nornctl status | grep compaction
compaction.runs: 2
compaction.segments: 2
compaction.saved_bytes: 0
compaction.failures: 0
```

That example saved nothing because its two segments are too far apart to merge and
hold too little to gain from it: an output stays within `max_segment_bytes` and
`max_segment_age`. The schedule is [`[maintenance]`](configuration.md#maintenance).

## Retention

A stream keeps everything unless you say otherwise. A policy is a span of time, a
number of calendar months, or a size. Age is measured from the stream's newest
sample, not the wall clock, so a stream gone quiet does not empty itself.

```console
$ nornctl retention set plant 14d
streams/plant max_age=1209600s
$ nornctl retention apply plant --dry-run
streams/plant would remove segments=1 samples=3 bytes=235 retained_bytes=470 cutoff=1970-01-21T17:20:02Z
0000000000000000-g0000.nseg samples=3 bytes=235 from=1970-01-12T13:46:40Z to=1970-01-12T13:46:42Z
$ nornctl retention apply plant
streams/plant removed segments=1 samples=3 bytes=235 retained_bytes=470 cutoff=1970-01-21T17:20:02Z
0000000000000000-g0000.nseg samples=3 bytes=235 from=1970-01-12T13:46:40Z to=1970-01-12T13:46:42Z
$ nornctl status | grep retention
retention.segments: 1
retention.samples: 3
retention.bytes: 235
retention.removal: streams/plant segments=1 samples=3 bytes=235
```

**Setting a policy is what authorises deleting.** Once set it also runs by itself,
hourly by default, so use `--dry-run` first. Only whole segments go, each entirely
past the limit, and the newest segment always stays, so a stream can hold more than
its policy says. Every removal is logged at `WARN` before and after:

```text
WARN retention removing segments stream=1 segments=1 samples=3 bytes=235 cutoff=Some(1790402000000000) policy=MaxAge(1209600s)
WARN retention removed segments stream=1 segments=1
```

`status` counts since the daemon started and lists the last 16 removals. The log is
the lasting record: keep it.

## Logs

To stderr; a supervisor owns files and rotation. `--log-format json` gives one
object per line, and `--log-level` sets the level
([configuration](configuration.md#logging)).

## Security

The socket's directory must be owner-only (0700) and the socket is created
owner-only; the daemon refuses to start otherwise.
[ipc.md](../reference/ipc.md#security-model-spec-41) has the model.

## Platforms

CI builds, lints and tests every push on Linux, macOS and Windows. On Windows the
tests run the daemon and `nornctl` over a named pipe, and so do the `compact`,
`verify` and `retention` test, which have type-checked for
Windows but not yet run on a Windows runner. Some tests run on Linux and macOS only,
and this is why:

| Test | Why not on Windows |
| --- | --- |
| `crates/server/tests/crash.rs`, `crash_reads.rs`, `query_damage.rs` | they `kill -9` a real `norn` child, through a helper (`tests/process/`) that listens on a Unix socket; not yet ported to a named pipe |
| `crates/server/tests/startup.rs` | uses the same helper to spawn and kill a real `norn`, to check what a start reads after a kill |
| `crates/server/tests/compaction_kill.rs` | kills a real daemon at every phase of a compaction; the child listens on a Unix socket, as the helper does |
| `crates/server/tests/signals.rs` | SIGTERM and SIGINT, which Windows does not have |
| `crates/server/tests/mcp_tools.rs`, `mcp_surface.rs` | MCP over a Unix socket, and `lsof` to prove no TCP port; MCP over a named pipe is served but not yet tested |
| `crates/cli/tests/exchange_memory.rs` | samples `nornctl`'s resident memory with `ps -o rss=`, which Windows lacks |
| `crates/cli/tests/history.rs` | written against a Unix socket; not yet ported to a named pipe |
| `crates/transport/tests/unix_socket.rs` | Unix sockets themselves; named pipes are tested on Windows instead |

Porting the kill tests is stage 16 work ([roadmap](../../README.md#roadmap)). Every
performance figure so far is from macOS.
