# Replay

Implemented: `crates/replay` (`norn-replay`, SPEC §25, §27, §28, stage 11), driven by
`crates/runtime` and served by `crates/server` as `ReplayService` and
`TelemetryService.SubscribeEvents`. The wire contract is in
[protocol.md](protocol.md#replay-spec-25-28); the how-to is the
[replay guide](../guides/replay.md). `StateAt(T)`, which a seek is built on, is in
[query.md](query.md#state-at-an-instant). This page is the design and the exact
semantics.

## Layering

| Crate | Owns | Must not |
| --- | --- | --- |
| `norn-replay` | `VirtualClock`, `Session` (one replay's state machine), the `Event` model, `ReplayError` | read a clock, spawn, know the wire |
| `norn-runtime` | `sessions::Replays` (a task per session, limits, expiry), `live::Live` (fan-out), `Subscription` | know the wire |
| `norn-server` | `ReplaySessions` (ReplayService), `Subscriptions` (SubscribeEvents) | reach storage but through the runtime |

A `Session` is told the wall time and answers with what to do next: emit these
events, wait this long, read the next page, or nothing until a control. It reads
pages through `norn-query`'s engine, so it sees chunks and buffered samples exactly
once, and survives compaction underneath it. The runtime acts on its answers with the
injected `Clock`, so every test of pacing, pausing and expiry runs on `FakeClock`.

## The virtual clock (SPEC §28)

`virtual = anchor + (wall - wall anchor) × speed` while running at a speed; every
control moves the anchor. It is independent of wall time: a paused session's clock
does not move however long it waits, and stepping takes no wall time at all.

| Pace | The clock | Events |
| --- | --- | --- |
| `speed = 0` | frozen; only a step moves it | only by step |
| `speed = 1`, `10`, `100`, any finite ≥ 0 | runs at that multiple | each when the clock reaches its `observe_time` |
| `max_speed` | jumps to each next event | as fast as the slowest subscriber reads |

Pacing arithmetic is `f64` nanoseconds, good to about 100 ns over a year of running.
No event's identity or order depends on it.

## Determinism (SPEC §25, §36)

A session emits the selected samples with `observe_time` in its half-open interval, in
`(observe_time, sequence, signal name)` order: `QuerySamples`' `BY_TIME` order. Which
sample events, in which order, depends only on the interval, the signals, and the
seeks and steps applied. Speed, pauses and how fast anyone reads change only *when*
events go out. A sample event's `clock_time` is its own `observe_time`.

Proved by:

- `crates/replay/tests/properties.rs`: random data laid out across segments, chunks and
  the write path's buffers; a replay at maximum speed, and one paced at a random speed
  and paused many times, both emit exactly the brute-force answer.
- `crates/server/tests/replay.rs`: over gRPC, a replay equals a `BY_TIME` query of the
  same interval sample for sample, and two replays give identical events and numbers.

The signals are resolved when the session is created; one created later does not join.
The interval may reach into the future: a sample arriving later that falls after the
session's position is replayed, one before it is not.

## States and controls

Sessions start `PAUSED` at the interval's start, so subscribers attach first.

| Control | From | To | Events sent |
| --- | --- | --- | --- |
| `:resume` | `PAUSED` (`RUNNING`: no-op) | `RUNNING` | `RUNNING` |
| `:pause` | `RUNNING` (`PAUSED`: no-op) | `PAUSED` | `PAUSED` |
| `:seek` | any but `STOPPED` | the same; `ENDED` becomes `PAUSED` | `SEEKED`, then state samples |
| `:step`, `event_count` N | `PAUSED`, or `RUNNING` at speed 0 | the same | the next N samples |
| `:step`, `duration` D | `PAUSED`, or `RUNNING` at speed 0 | the same | the samples before clock + D |
| `UpdateReplaySession` (pace) | any but `STOPPED` | the same | `SPEED_CHANGED` |
| `:stop`, `Delete` | any (`STOPPED`: no-op) | `STOPPED` | `STOPPED`, then the stream closes |
| the clock reaching the end | `RUNNING`, or a step | `ENDED` | `ENDED` |

Anything else fails with `FAILED_PRECONDITION` and `ERROR_REASON_REPLAY_STATE_CONFLICT`,
as does any control but stop while a step is still delivering. A step answers once its
events have reached every subscriber; a step that runs past the end ends the session.
`ENDED` does not close a subscription: a seek brings more.

**A seek reports the state.** Right after `SEEKED`, each selected signal that had a
sample before the seek's target gets one `state_sample` event: its newest sample at or
before `target - 1 ns`, from `StateAt`. The sample at exactly the target is replayed as
an ordinary sample next. So folding a subscription's samples and state samples, keeping
each signal's last, gives the state at its clock across any seeks: SPEC §36's invariant,
live in the stream. A seek with no subscriber attached skips the lookup. `emitted_count`
restarts at 0, and counts sample events only.

**Etags** are `"g<generation>"`; the generation changes with every control and state
change. A control with a stale etag fails with `ABORTED` and `ERROR_REASON_ETAG_MISMATCH`.

## Subscriptions (SPEC §27)

`SubscribeEvents` carries the same `Event` for `LIVE` and `REPLAY`. Each subscription
numbers its own events 1, 2, 3, ... with no gap, and no event is ever skipped: a
subscriber that cannot keep up is ended, with a reason, after everything it had.

| | `LIVE` | `REPLAY` |
| --- | --- | --- |
| Source | samples committed after it starts, of the signals `parent` and `filter` chose when it started | one session |
| `clock_time` | the daemon's clock when delivered | the session's virtual clock |
| Payloads | `sample` | `sample`, `state_sample`, `clock_event` |
| A slow reader | is ended with `RESOURCE_EXHAUSTED`, `ERROR_REASON_SUBSCRIBER_TOO_SLOW` | slows the session's clock: it waits for the slowest |
| Ends | at shutdown, or too slow | `STOPPED` (then closes), a read failure, shutdown |

**Live never slows ingestion.** The commit thread hands each committed batch to every
live subscriber with a non-blocking send of a shared reference; filtering happens on the
subscriber's own task. With no subscriber it costs one atomic load. A subscriber's queue
holds 64 batches; when it is full, that subscriber is ended as too slow.
`crates/server/tests/live.rs` appends 40,000 samples past a subscriber that never reads:
every batch is acknowledged, a reading subscriber gets all 40,000, and the idle one ends
with `SUBSCRIBER_TOO_SLOW` after a gapless prefix.

A live sample is delivered after its batch is journalled and buffered and before the
journal is flushed, so a subscriber can see a sample that a crash then loses, as a
query can ([query.md](query.md#freshness-no-gap-no-double)). Duplicates refused at
admission are never delivered.

## Bounds (SPEC §16, §41)

| What | Bound | When passed |
| --- | --- | --- |
| Sessions | `Limits::max_replay_sessions` (16) | `RESOURCE_EXHAUSTED`, `CAPACITY_EXHAUSTED`, `limit=max_replay_sessions` |
| Idle time | `replay_session_idle_timeout` (10 min) since the last call naming the session; one with a subscriber is never idle | deleted; a later call gets `NOT_FOUND` |
| Subscriptions, both modes | `Limits::max_subscriptions` (256) | `RESOURCE_EXHAUSTED`, `CAPACITY_EXHAUSTED`, `limit=max_subscriptions` |
| A session's reads | the replay budget (`replay_bytes`) shared among `max_replay_sessions`: a slice of at most half a share, a page of at most a quarter, 4 open files | the query engine narrows, then refuses, typed |
| Events per message | 1,024 | split |
| Queued per replay subscriber | 4 batches, each charged until every subscriber has it | the clock waits |
| Queued per live subscriber | 64 batches, charged once to `replay_bytes` | ended as too slow; a full budget ends the subscribers holding it |
| Requests queued per session | 16 | callers wait |

Sessions are not stored: a restart ends them all. Shutdown stops every session and joins
its task before the daemon exits.

## Failure

- A damaged chunk stops the read with `DATA_LOSS`, `ERROR_REASON_DATA_CORRUPTED`. The
  session pauses before it; what was emitted is exact, because a page is read whole
  before any of it goes out. Subscribers are ended with that status, and whoever was
  stepping gets it too. A seek past the damage carries on.
- Compaction or retention moving files under a replay costs a re-read, never an event:
  the cursor remembers a position, not a chunk. `crates/replay/tests/storage.rs`
  compacts between steps and compares with an undisturbed replay.
- After `kill -9`, a replay of everything returns every sample acknowledged `DURABLE`,
  once and in order (`crates/server/tests/crash_reads.rs`).

## Measured

Replay at maximum speed to one subscriber over gRPC on a Unix socket delivered 262,000
samples a second, every one of them. [performance.md](../performance.md#replay) has the
method and the 16,000-signal run.

## Not implemented

- Sessions surviving a restart.
- Filtering by value or quality inside a session (`QuerySamples`' `sample_filter`).
- A live subscription following signals created after it starts.
