<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Querying

Read samples and aggregates back with `nornctl query`, or `QueryService.QuerySamples`
from your own client. Output here is real, from one session against a daemon holding
one `double` signal and one `int64` signal. Exact semantics, including NaN and
counter resets, are in [query.md](../reference/query.md).

## Read a range

A sample is queryable as soon as its append is acknowledged at `PERSISTED`. Nothing
waits for a chunk:

```console
$ nornctl query entities/dev-01/signals/temperature
2026-11-02T01:46:44.355065Z entities/dev-01/signals/temperature 21.5 GOOD seq=1
2026-11-02T01:46:44.643901Z entities/dev-01/signals/temperature 21.75 GOOD seq=2
2026-11-02T01:46:44.924409Z entities/dev-01/signals/temperature 22 GOOD seq=3
2026-11-02T01:46:45.206721Z entities/dev-01/signals/temperature 22.25 GOOD seq=4
2026-11-02T01:46:45.485042Z entities/dev-01/signals/temperature 80 GOOD seq=5
2026-11-02T01:46:45.755153Z entities/dev-01/signals/temperature 22.5 GOOD seq=6
$ nornctl streams
streams/plant samples=7 buffered=7
```

`buffered` says how many have not reached a chunk yet. A minute later they have, and
the answers are the same:

```console
$ nornctl streams
streams/plant samples=7
$ nornctl query entities/dev-01/signals/temperature --agg count
- - entities/dev-01/signals/temperature 6 count=6
```

`TARGET` is a signal, an entity (`dev-01` or `entities/dev-01`), or `all`. Bound the
time with `--from` and `--to`, half-open: RFC 3339 in UTC, Unix seconds, `now`, or
`now-5m`.

## Filter

`--where` is the sample filter. `--select` filters signals, as ListSignals does:

```console
$ nornctl query entities/dev-01/signals/temperature --where "value < 50 AND quality.severity = QUALITY_SEVERITY_GOOD" --agg average
- - entities/dev-01/signals/temperature 22 count=5
```

The spike of 80 is gone from that average. Quality never changes a value: a `BAD`
sample is history and is returned unless you filter it out.

Mistakes are refused, never read as "no match":

```console
$ nornctl query entities/dev-01/signals/temperature --where "value >> 1"
nornctl: QueryService.QuerySamples: InvalidArgument: sample_filter: `> 1` is not a literal (norn.local ERROR_REASON_INVALID_ARGUMENT) field=sample_filter
$ nornctl query entities/dev-01/signals/temperature --agg sum --where 'value > "a"'
nornctl: QueryService.QuerySamples: InvalidArgument: sample_filter: entities/dev-01/signals/temperature: sample_filter compares value with something of another type (norn.local ERROR_REASON_INVALID_ARGUMENT) field=sample_filter
$ nornctl query entities/nope
nornctl: QueryService.QuerySamples: NotFound: Entity nope not found (norn.local ERROR_REASON_RESOURCE_NOT_FOUND) resource=nope
```

## Aggregate

`--agg` is `latest`, `first`, `last`, `count`, `min`, `max`, `average`, `sum` or
`rate`. `--window` sets a fixed width, the sampling interval; without one there is one
window over the whole range. Lines are `start end signal value count=n`, with `-` for
an open end. Windows align to `--from`, or to the Unix epoch without one:

```console
$ nornctl query entities/dev-01/signals/temperature --agg count --window 1s
2026-11-02T01:46:44Z 2026-11-02T01:46:45Z entities/dev-01/signals/temperature 3 count=3
2026-11-02T01:46:45Z 2026-11-02T01:46:46Z entities/dev-01/signals/temperature 3 count=3
$ nornctl query entities/dev-01/signals/temperature --agg max --from now-5m --to now
2026-11-02T01:41:46.069477Z 2026-11-02T01:46:46.069479Z entities/dev-01/signals/temperature 80 count=6
```

`latest` gives one line per signal, and its window is the sample's own instant:

```console
$ nornctl query dev-01 --agg latest
2026-11-02T01:46:45.755153Z 2026-11-02T01:46:45.755153001Z entities/dev-01/signals/temperature 22.5 count=1
2026-11-02T01:46:46.031279Z 2026-11-02T01:46:46.031279001Z entities/dev-01/signals/valve 40 count=1
```

`rate` is for counters. It treats any decrease as a reset, so on this temperature,
whose spike falls from 80 back to 22.5, it counts that as a reset and gives nonsense.
For a gauge use `first` and `last`:

```console
$ nornctl query entities/dev-01/signals/temperature --agg rate
- - entities/dev-01/signals/temperature 57.85350635102937 count=6
```

## Stop and resume

`--order time` merges signals by time, the order replay uses; the default groups them
by signal, which is cheaper. `--limit N` stops at the first response that reaches `N`
results and prints a token. Responses hold up to 10,000 results, so **`--limit` can
print more than `N`**, as here, where `--limit 1` printed one response of seven:

```console
$ nornctl query dev-01 --order time --limit 1
2026-11-02T01:46:44.355065Z entities/dev-01/signals/temperature 21.5 GOOD seq=1
2026-11-02T01:46:44.643901Z entities/dev-01/signals/temperature 21.75 GOOD seq=2
2026-11-02T01:46:44.924409Z entities/dev-01/signals/temperature 22 GOOD seq=3
2026-11-02T01:46:45.206721Z entities/dev-01/signals/temperature 22.25 GOOD seq=4
2026-11-02T01:46:45.485042Z entities/dev-01/signals/temperature 80 GOOD seq=5
2026-11-02T01:46:45.755153Z entities/dev-01/signals/temperature 22.5 GOOD seq=6
2026-11-02T01:46:46.031279Z entities/dev-01/signals/valve 40 GOOD seq=1
resume_token: q1.c44fa26b95b0edec.s.1793584006031279000.1.entities/dev-01/signals/valve
```

Pass it back with `--resume TOKEN` and otherwise equal flags to get exactly the
remainder; here that would print nothing. A token from a query with other flags is
refused. Relative times count as other flags, since `now` moves.

## Your own client

[`crates/cli/examples/query_client.rs`](../../crates/cli/examples/query_client.rs) uses
only `norn-ipc` and `norn-protocol`, reading an aggregate and resuming a raw read:

```console
$ cargo run -p norn-cli --example query_client -- unix:/tmp/nq.L87l/run/norn/norn.sock entities/dev-01 temperature
entities/dev-01/signals/temperature Some(Value { kind: Some(DoubleValue(31.666666666666668)) }) over 6 samples
6 samples
```

A broken stream loses nothing: reconnect, and resend the request with the last
`resume_token` you received.

## When a query fails

| Status | Reason | Means |
| --- | --- | --- |
| `DATA_LOSS` | `ERROR_REASON_DATA_CORRUPTED` | a chunk failed its checksum. What arrived before is exact. Metadata gives `min_time` and `max_time`: query around them. See [operations](operations.md#damage) |
| `RESOURCE_EXHAUSTED` | `ERROR_REASON_CAPACITY_EXHAUSTED` | the query budget stayed full for a second. Retry, resuming, or raise `query_bytes` |
| `RESOURCE_EXHAUSTED` | `ERROR_REASON_LIMIT_EXCEEDED` | one instant holds more than one read can take. Select fewer signals |
| `INVALID_ARGUMENT` | `ERROR_REASON_INVALID_TOKEN` | the token belongs to a different request |
| `UNAVAILABLE` | `ERROR_REASON_SHUTTING_DOWN` | the daemon is stopping. Resume after it restarts |

Latency and throughput are in [performance](../performance.md#query).
