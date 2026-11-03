<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Replay and state

What a system looked like at any instant, and its history played back as if it were
live: deterministic, on a clock you control, through the same subscription as live data.

## State at an instant

`nornctl state` asks `StateService.ComputeState`: for each signal, the latest sample at
or before the instant. This is a real session against a fresh daemon:

```console
$ nornctl streams create plant
streams/plant
$ nornctl entities create ahu-01
entities/ahu-01
$ nornctl signals create ahu-01 supply-temp plant double
entities/ahu-01/signals/supply-temp
$ nornctl signals create ahu-01 fan-speed plant double
entities/ahu-01/signals/fan-speed
$ nornctl append plant entities/ahu-01/signals/supply-temp 21.5
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_DURABLE
$ nornctl append plant entities/ahu-01/signals/supply-temp 21.9 --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
$ nornctl append plant entities/ahu-01/signals/fan-speed 40 --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
$ nornctl state ahu-01
state_time: 2026-11-02T02:50:13.233494Z
2026-11-02T02:50:13.229454Z entities/ahu-01/signals/fan-speed 40 GOOD seq=0
2026-11-02T02:50:13.224455Z entities/ahu-01/signals/supply-temp 21.9 GOOD seq=0
$ nornctl state ahu-01 --at now-2s
state_time: 2026-11-02T02:50:11.237578Z
- entities/ahu-01/signals/fan-speed unknown
2026-11-02T02:50:11.174288Z entities/ahu-01/signals/supply-temp 21.5 GOOD seq=0
$ nornctl state entities/ahu-01/signals/supply-temp --max-age 1s
state_time: 2026-11-02T02:50:13.241634Z
2026-11-02T02:50:13.224455Z entities/ahu-01/signals/supply-temp 21.9 GOOD seq=0
$ nornctl state ahu-01 --at 2020-01-01T00:00:00Z
state_time: 2020-01-01T00:00:00Z
- entities/ahu-01/signals/fan-speed unknown
- entities/ahu-01/signals/supply-temp unknown
$ nornctl state ahu-02
```

What to notice:

- A signal with no sample yet at the instant is `unknown`, not left out: every selected
  signal gets a line, in name order.
- `--at` takes RFC 3339 in UTC, Unix seconds, `now` or `now-D`; a sample at exactly that
  instant counts. With no `--at` it is now.
- A sample is in the state the moment it is acknowledged at `PERSISTED`, buffered or not:
  the samples above were seconds old and still in memory. Two seconds before the second
  call, `supply-temp` still read 21.5 and `fan-speed` did not exist yet.
- `--max-age D` turns a sample older than D into `unknown`: here the sample was younger
  than a second, so it shows. Use it when a stale reading would mislead.
- An entity that does not exist is an error, with a reason a program can match on.

A program calls `ComputeState` directly. A large answer comes in pages, and the page token
pins the answer, so every page describes the same instant even if samples arrive between
calls. A token lives 60 s after its last use; past that, start again.

A damaged chunk is `DATA_LOSS` with `ERROR_REASON_DATA_CORRUPTED`, never a signal shown as
`unknown`. The [troubleshooting guide](troubleshooting.md) covers reading around damage.

How fast it is, and why there are no checkpoints:
[storage.md](../reference/storage.md#state-checkpoints). Exact semantics:
[query.md](../reference/query.md#state-at-an-instant).

## Replay

A replay session plays a stretch of history on a virtual clock. It starts paused, so you
subscribe first; then you resume it, pause it, seek, step, and change its speed. Every
command below ran against one real daemon, after these samples were appended with
`--at` (output trimmed to the first; the rest answered the same):

```console
$ nornctl streams create plant
streams/plant
$ nornctl entities create ahu-01
entities/ahu-01
$ nornctl signals create ahu-01 supply-temp plant double
entities/ahu-01/signals/supply-temp
$ nornctl signals create ahu-01 fan-speed plant double
entities/ahu-01/signals/fan-speed
$ nornctl append plant entities/ahu-01/signals/supply-temp 21.0 --at 2026-11-02T08:00:00Z --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
(five more appends like it, at 08:00:00, :10, :20, :30 and :40)
$ nornctl state ahu-01 --at 2026-11-02T08:00:15Z
state_time: 2026-11-02T08:00:15Z
2026-11-02T08:00:00Z entities/ahu-01/signals/fan-speed 40 GOOD seq=0
2026-11-02T08:00:10Z entities/ahu-01/signals/supply-temp 21.4 GOOD seq=0
```

### Play it

`--speed max` plays as fast as the subscriber reads; `--speed 1` is real time, `10` ten
times faster, `0` holds the clock still so only steps move it.

```console
$ nornctl replay create ahu-01 --from 2026-11-02T08:00:00Z --to 2026-11-02T08:01:00Z --speed max --id morning
replaySessions/morning PAUSED at=2026-11-02T08:00:00Z emitted=0 pace=max from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl subscribe replay morning --resume
#1 2026-11-02T08:00:00Z RUNNING
#2 2026-11-02T08:00:00Z 2026-11-02T08:00:00Z entities/ahu-01/signals/fan-speed 40 GOOD seq=0
#3 2026-11-02T08:00:00Z 2026-11-02T08:00:00Z entities/ahu-01/signals/supply-temp 21 GOOD seq=0
#4 2026-11-02T08:00:10Z 2026-11-02T08:00:10Z entities/ahu-01/signals/supply-temp 21.4 GOOD seq=0
#5 2026-11-02T08:00:20Z 2026-11-02T08:00:20Z entities/ahu-01/signals/fan-speed 55 GOOD seq=0
#6 2026-11-02T08:00:30Z 2026-11-02T08:00:30Z entities/ahu-01/signals/supply-temp 21.9 GOOD seq=0
#7 2026-11-02T08:00:40Z 2026-11-02T08:00:40Z entities/ahu-01/signals/fan-speed 60 GOOD seq=0
#8 2026-11-02T08:01:00Z ENDED
```

- Each line is one event: its number, the clock when it happened, and what it was.
  Numbers start at 1 and never skip.
- A sample event is timed by its own `observe_time`, and samples come in time order,
  then by sequence, then by signal name: the same order every run, at every speed.
- `ENDED` does not close a subscription, since a seek brings more. `nornctl subscribe`
  stops there; a program can keep listening.

### Seek and step

A seek moves the clock without playing what it skips, and then tells subscribers what
each signal held just before the new time, from `StateAt`. Folding a subscription's
samples and `state` lines, keeping each signal's last, always gives the state at the
clock. Here a second subscriber watches while the session is moved by hand:

```console
$ nornctl subscribe replay morning --limit 5 &
$ nornctl replay seek morning 2026-11-02T08:00:15Z
replaySessions/morning PAUSED at=2026-11-02T08:00:15Z emitted=0 pace=max from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl replay step morning 2
replaySessions/morning PAUSED at=2026-11-02T08:00:30Z emitted=2 pace=max from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
#1 2026-11-02T08:00:15Z SEEKED
#2 2026-11-02T08:00:15Z state 2026-11-02T08:00:00Z entities/ahu-01/signals/fan-speed 40 GOOD seq=0
#3 2026-11-02T08:00:15Z state 2026-11-02T08:00:10Z entities/ahu-01/signals/supply-temp 21.4 GOOD seq=0
#4 2026-11-02T08:00:20Z 2026-11-02T08:00:20Z entities/ahu-01/signals/fan-speed 55 GOOD seq=0
#5 2026-11-02T08:00:30Z 2026-11-02T08:00:30Z entities/ahu-01/signals/supply-temp 21.9 GOOD seq=0
```

A step of `N` plays the next N samples; a step of a duration, such as `10s`, moves the
clock that far and plays every sample before the new time. Both leave the session
paused, and answer once every subscriber has the events.

```console
$ nornctl replay step morning 10s
replaySessions/morning PAUSED at=2026-11-02T08:00:40Z emitted=2 pace=max from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl replay speed morning 10
replaySessions/morning PAUSED at=2026-11-02T08:00:40Z emitted=2 pace=10x from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl replay list
replaySessions/morning PAUSED at=2026-11-02T08:00:40Z emitted=2 pace=10x from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl replay stop morning
replaySessions/morning STOPPED at=2026-11-02T08:00:40Z emitted=2 pace=10x from=2026-11-02T08:00:00Z to=2026-11-02T08:01:00Z
$ nornctl replay resume morning
nornctl: ReplayService: FailedPrecondition: cannot resume a session that is stopped (norn.local ERROR_REASON_REPLAY_STATE_CONFLICT)
$ nornctl replay delete morning
deleted replaySessions/morning
```

A control the session's state forbids is refused with
`ERROR_REASON_REPLAY_STATE_CONFLICT`. Sessions are kept in memory: at most 16, each
deleted after 10 minutes with no call and no subscriber, and all of them at a restart.

## Live

`subscribe live` follows samples as they are committed, in the same event format, so a
consumer written for one runs unchanged on the other. The clock is the daemon's:

```console
$ nornctl subscribe live ahu-01 --limit 2 &
$ nornctl append plant entities/ahu-01/signals/supply-temp 22.3 --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
$ nornctl append plant entities/ahu-01/signals/fan-speed 62 --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
#1 2026-11-02T04:06:13.351223Z 2026-11-02T04:06:13.350664Z entities/ahu-01/signals/supply-temp 22.3 GOOD seq=0
#2 2026-11-02T04:06:13.358668Z 2026-11-02T04:06:13.358324Z entities/ahu-01/signals/fan-speed 62 GOOD seq=0
```

A live subscriber that stops reading is never shown a gap and never slows ingestion: when
its queue of 64 batches fills, it is ended with `ERROR_REASON_SUBSCRIBER_TOO_SLOW`.
Resubscribe, and query the interval you missed.

## More

- Exact rules, bounds and failure behaviour: [replay.md](../reference/replay.md).
- The wire contract: [protocol.md](../reference/protocol.md#replay-spec-25-28).
- Every flag: [nornctl](../reference/cli.md#replay).
- Speed: replay at maximum speed delivered 1.15 million samples a second to one
  subscriber at 16,000 signals ([performance](../performance.md#replay)).
