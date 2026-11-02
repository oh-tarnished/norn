# Operations

Working today: starting, stopping, recovery after a crash, and reporting damage.
**Not available to an operator yet:** compaction (the format code exists, nothing
in the daemon runs it), retention (candidates can be listed in code, nothing
deletes), export and import, `verify`, metrics, backup tooling. All are planned;
see the [roadmap](../../README.md#roadmap).

Output below is real, from macOS. Linux and Windows have not been run.

## Stopping

SIGINT or SIGTERM drains in-flight calls for up to `drain_timeout_ms`, then
exits 0.

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
norn/streams/0000000000000001/journal/LOCK
norn/streams/0000000000000001/LOCK
```

`metadata/` is the registry of streams, entities and signals. `streams/<id>/` holds
sealed segments, and `journal/` the samples not yet stored as chunks. `index/`
can be rebuilt from the segments. One daemon owns a directory at a time; a second
one stops at the lock:

```console
$ norn --listen unix:/tmp/norn-demo/n2.sock --data-dir /tmp/norn-demo/data
2026-11-01T21:19:28.862780Z ERROR cannot start error=another process holds /tmp/norn-demo/data/metadata/LOCK
```

## Crash recovery

Nothing needs doing: recovery runs at every start. Here one `DURABLE` sample is
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

## Damage

A checksum failure is reported and never silently repaired. Here three bytes of a
sealed segment were overwritten by hand before a restart:

```console
$ nornctl status | grep recovery
recovery.streams: 2
recovery.events: 1
recovery.corruptions: 1
recovery.replayed_samples: 0
```

The log says where, at `WARN`:

```text
WARN recovery found damaged data recovery=IngestRecovery { ... findings: ["Corruption { corruption: Corruption { path: \"/tmp/norn-demo/x/norn/streams/0000000000000001/0000000000000000-g0000.nseg\", offset: 48, len: 129, kind: BodyChecksum { stored: 1554215434, computed: 2126562449 } }, action: LeftInPlace }"], ... }
```

Three things to know:

- The file stays in place and the report repeats at every start.
- `nornctl streams` leaves the damaged chunk's samples out of `samples` and shows
  them as `corrupt=N`. That reflects the last start: a chunk damaged while the daemon
  runs still counts until the next one.
- A query reaching the chunk fails with `DATA_LOSS`, naming it and its time span, and
  never skips it. [Query around it](querying.md#when-a-query-fails): everything else
  stays readable.
- Nothing reads quarantined files yet. There is no repair or salvage tool.

Every start verifies all stored chunks, which is fine for hours of data and not
yet for a year ([storage.md](../reference/storage.md#not-implemented)).

## Logs

To stderr; a supervisor owns files and rotation. `--log-format json` gives one
object per line, and `--log-level` sets the level
([configuration](configuration.md#logging)).

## Security

The socket's directory must be owner-only (0700) and the socket is created
owner-only; the daemon refuses to start otherwise.
[ipc.md](../reference/ipc.md#security-model-spec-41) has the model.
