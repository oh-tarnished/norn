# Query

Implemented in `crates/query` (`norn-query`, SPEC §22, stage 9) and served as
`QueryService.QuerySamples` by `crates/server`. The wire contract is in
[protocol.md](protocol.md#query-spec-22); the how-to is the
[querying guide](../guides/querying.md). This page is the design and the exact
semantics.

## Layering

`norn-query` imports core, codec, storage and index. It may not import the runtime,
so two things arrive through traits it declares:

| Trait | Implemented by | Why |
| --- | --- | --- |
| `TailSource` | `norn_runtime::ingest::Ingest` | samples not in the index yet |
| `QueryMemory` | `norn_runtime::Budget` | the query budget (`query_bytes`) |

`norn-server` may not import `norn-query` either, so it reaches the engine through
`norn_runtime::query`, a re-export, and `Ingest::query_engine`. Selecting signals
stays the catalogue's job: the server asks `norn-metadata` and hands in `Target`s.

## One read

Everything is built on one operation: read some signals over one time slice.

1. Copy the matching rows out of the tail.
2. **Then** take an index snapshot, plan, and read each chunk, clipping it: chunks
   overlap a range, and are not cut to it.
3. Sort by (time, sequence, signal) and collapse equal rows.

A slice is read whole before any of it is delivered. That one fact gives three
guarantees cheaply:

- **Races.** `ChunkFile::open` failing `NotFound`, or a plan's `SegmentGone`, means
  compaction or retention moved the data. Nothing from this slice has gone out, so
  the read calls `refresh`, takes a new snapshot and reads the slice again, up to
  16 times. No per-signal bookkeeping, and no locator outlives its snapshot.
- **Corruption.** A slice holding a damaged chunk delivers nothing, so what was
  delivered before the error is exact.
- **Order under overlap.** Late data overlaps; sorting a bounded slice handles it.

## Freshness: no gap, no double

The most important property: **a sample acknowledged at `PERSISTED` or better is
visible to the next query.** A chunk may not exist for a minute, so a query cannot
wait for one. A sample's life, in `crates/runtime/src/ingest`:

```text
commit thread: journal, then buffers ─ PERSISTED ─ seal: buffers → frozen, one write lock
store thread:  encode, write, fsync, tell the index ─ then retire from frozen
```

- A sample enters the tail before `PERSISTED`, and moves from buffers to a frozen
  generation under one lock, so a reader never finds it in neither.
- It leaves the tail only **after** the index has it. If the index falls behind, it
  stays, its memory still reserved.

So the time it is in the tail and the time it is in the index overlap, and never
leave a gap between them. A reader that takes the tail **first** and the snapshot
**second** must see it in at least one: had it left the tail by then, it was already
in the index when the snapshot was taken. Sometimes it sees it in both, and a chunk
of a generation can reach the index before the rest of it does; step 3 collapses
those. `crates/runtime/tests/freshness.rs` queries in a loop through forced flushes,
and `crates/server/tests/query.rs` through real ones every 20 ms.

A sample can be seen slightly before its `PERSISTED`: it is in the buffer just
before the journal flush returns. A crash in that instant loses it, as it loses any
unacknowledged sample. At `BUFFERED`, a sample may still be in the admission queue,
so `BUFFERED` promises nothing about visibility.

Collapsing also tidies a case the write path allows: a resend older than the dedup
window is stored twice ([storage.md](storage.md#duplicates-exactly-what-is-guaranteed)).
**A query returns each (signal, time, sequence) once.** If two stored rows share
that key but differ in value, which only a misbehaving client can cause, which one
wins is not defined.

## Bounded memory

Every row, decoded chunk, undelivered page, frame buffer and signal name is charged
to the query budget **before** it is held. `Charge` returns it on drop. A page stays
charged until tonic takes it to encode.

| What | Bound |
| --- | --- |
| Rows of one slice | `query_bytes / 8` (8 MiB by default), about 110,000 rows |
| One decoded chunk | 26 bytes a sample; a string or bytes chunk charges 16 MiB more, its largest possible block, then frees it |
| Page | `max_query_page_samples` (10,000), or about 2 MiB, under gRPC's default 4 MiB message limit. One larger result travels alone |
| Open files | 16 per query, least recently used evicted, each with its largest frame |
| Pages queued for a client | 2 |

**Slices.** A plan's metadata says how many samples each chunk holds, so the slicer
estimates how many fall in a candidate range, assuming they are evenly spread, and
shrinks the range until it fits. It reads sidecars and no segment. A wrong guess
costs a retry: rows passing the cap stop the read, which halves the range and goes
again. That also handles the tail, which the estimate cannot see, and `PlanTooLarge`.
The next slice tries double. One instant cannot be cut, so there only the budget
binds, and failing it is a typed stop.

Costs of this design, measured or not:

- 16,000 hour-long chunks read in time order are decoded once per slice they
  overlap. The time order is the expensive order. `chunk_cache_bytes` exists for a
  decoded-chunk cache, which is not built.
- COUNT decodes chunks; it could use chunk metadata when no filter applies, except
  that collapsing duplicates needs the rows.
- Chunks are decoded one at a time, on one thread per query.

**A slow client.** `QuerySamples` is a task that pulls a page on the blocking pool
only once the client has room. A client that stops reading holds its queued pages'
charge and no thread, and stalls nobody else until the budget is full; others then
wait about a second and fail with `RESOURCE_EXHAUSTED`,
`ERROR_REASON_CAPACITY_EXHAUSTED`. Limit: nothing cancels a query whose client is
connected and silent, short of the 10-minute idle close.

A scan of the tail never waits for memory, because it holds a lock the commit thread
needs: if the budget is short there, the slice narrows, then fails typed.

## Semantics

### Selection and range

- `parent` is `entities/{entity}` or `entities/-`. `filter` is ListSignals', plus
  `id`, the last part of a signal's name.
- `interval` is half-open on `observe_time`. Unset ends are open. `start > end` is
  `INVALID_ARGUMENT`; `start = end` is empty.
- Order: `BY_SIGNAL` is (signal name, time, sequence). `BY_TIME` is (time, sequence,
  signal name). Aggregates are (signal name, window).

### Sample filter

Clauses joined by `AND`; `OR`, `NOT` and parentheses are refused.

| Field | Operators | Right-hand side |
| --- | --- | --- |
| `value` | `= != < <= > >=` | a number, `true` or `false`, or `"text"` |
| `quality.severity` | the same, `GOOD < UNCERTAIN < BAD` | `QUALITY_SEVERITY_GOOD` ... |
| `quality.substatus` | the same, by number | `QUALITY_SUBSTATUS_STALE` ... |

- A comparison with a type no selected signal could hold, such as `value > 20` on a
  string signal, or `<` on a boolean, fails the whole query. It is never a silent
  non-match.
- Numbers compare exactly between integers. An integer against a fractional literal
  compares as `f64`. An `f32` value is widened to `f64` first, so `value = 0.1` is
  false for the `f32` nearest 0.1.
- `NaN` passes `!=` and nothing else. `-0.0 = 0.0` holds.
- Enums compare by number. Strings and bytes compare bytewise.

### Windows

- Fixed width, aligned to the interval's start, or to the Unix epoch when it has
  none, so a window's identity never depends on the data. The last is clipped to
  the interval's end.
- No width means one window over the whole interval, whose bounds are the interval's
  own, unset where it is unset.
- An empty window produces nothing. The window width is the "sampling interval" of
  SPEC §22: `FIRST` or `LAST` over a window downsamples.

### Aggregates

Samples feed a window in (time, sequence) order. The filter runs first.

| Function | Result | Result type |
| --- | --- | --- |
| `LATEST` | newest matching sample of each signal in the interval; `window` is that sample's instant, one nanosecond wide | the signal's |
| `FIRST`, `LAST` | earliest, latest | the signal's, bit-exact |
| `COUNT` | samples | `int64` |
| `MIN`, `MAX` | see below | the signal's, bit-exact |
| `AVERAGE` | `SUM / COUNT` | `double` |
| `SUM` | see below | `double` |
| `RATE` | see below | `double` |

`AVERAGE`, `SUM` and `RATE` need `int32`, `int64`, `uint32`, `uint64`, `float` or
`double`; any other selected signal fails the query with `INVALID_ARGUMENT`. `bool`
and `enum` are not numbers here.

**NaN, infinities and signed zero.**

| Case | MIN, MAX | SUM, AVERAGE |
| --- | --- | --- |
| Order | IEEE 754 `totalOrder`: `-NaN < -inf < ... < -0.0 < +0.0 < ... < +inf < +NaN` | none: added in (time, sequence) order |
| A `NaN` in the window | `MAX` returns a positive NaN, `MIN` a negative one; the opposite end ignores it | NaN |
| `+inf` and `-inf` together | each end returns its own | NaN |
| `-0.0` and `+0.0` | `MIN` gives `-0.0`, `MAX` gives `+0.0`, in either order | `+0.0`; all `-0.0` sums to `-0.0`, because the first sample starts the sum |

`totalOrder` was chosen so a result depends on no sample's position, and a NaN is
never silently dropped. A NaN result's payload is unspecified. A finite sum that
overflows is an infinity.

Integers sum exactly in `i128`, then round once to `double`, so `i64::MAX + i64::MAX -
i64::MAX` is `i64::MAX` as a double, not a precision loss in the middle. Float sums
are plain IEEE additions: uncompensated, so the rounding of a very long sum is what
adding in that order gives. Results are deterministic for a given set of samples.

`MIN` and `MAX` also order booleans (`false` first), enums by number, and strings and
bytes bytewise.

**RATE** is a counter's increase per second, the question `RATE` is asked for:

```text
increase = sum over consecutive samples p, c of:  c - p   if c >= p
                                                  c       if c < p (a reset)
rate     = increase / seconds from the first sample to the last
```

- A decrease is a counter reset, and the counter is taken to have restarted from
  zero, so that step adds the new value. Increase between the last sample before the
  reset and the reset itself cannot be known and is not guessed.
- **So `RATE` of a gauge is wrong.** A temperature that falls looks like a reset. For
  a gauge's change, use `LAST` and `FIRST`.
- Fewer than two samples, or all at one instant, produces no result.
- Integers compute exactly, so a `uint64` counter near its top keeps its steps.
- A window's rate sees only that window, and only samples passing the filter: an
  increase across a window boundary belongs to neither.
- A NaN gives NaN. Infinities follow IEEE arithmetic on the steps.

### Resume tokens

Each response carries a token for its last result. It is opaque, and bound by a
fingerprint to every other field of the request, so changing any of them fails with
`INVALID_ARGUMENT`, `ERROR_REASON_INVALID_TOKEN`. It names a signal, plus a time and
sequence or a window index, never a chunk, so compaction does not invalidate it.

A resumed query returns **exactly the remainder** of the same data. Live data arriving
meanwhile appears if it falls after the position; a late sample before it does not. A
token outlives a restart, and is valid while the history it points into is retained.
It resumes at page granularity: the smallest step is a response.

### Corruption

A chunk failing its checksum stops the query with `DATA_LOSS`,
`ERROR_REASON_DATA_CORRUPTED`, whose `ErrorInfo` metadata has `chunk`, `signal_id`,
`min_time` and `max_time`. It is never skipped, even if the chunk would contribute
nothing after the filter. To read around it, query the intervals before `min_time` and
after `max_time`, or select other signals.

`Stream.stats.sample_count` excludes samples in chunks that failed verification at the
last start, and `corrupt_sample_count` holds them. Honest limits: the check is
startup's, so a chunk damaged while the daemon runs is still counted until the next
start, though a query reports it at once. The number is what the segment's directory
says the chunk held.

## The last-seen summary

LATEST and StateAt ask "where is this signal's newest chunk?". The index
used to walk every segment for a silent or absent signal. `norn-index` now keeps a
bounded per-signal summary, `Snapshot::last_seen`, in
[storage.md](storage.md#the-per-signal-summary).

## State at an instant

`StateService.ComputeState`, SPEC's `StateAt(T)` (§23, §26), is built in
`crates/state` (`norn-state`) on this engine, and served by `crates/server` through
`norn_runtime::state`. The wire contract is in
[protocol.md](protocol.md#state-at-an-instant-spec-23-26).

### Semantics

- For each selected signal, the sample with the greatest (time, sequence) whose time is
  at or before `state_time`. A sample at exactly `state_time` counts.
- With `max_age`, a sample older than `state_time - max_age` is reported as no sample:
  unknown, not stale. One exactly that old is kept. A negative `max_age` is
  `INVALID_ARGUMENT`.
- Every selected signal has an entry, in signal name order, with no sample if it has
  none yet.
- **The invariant (SPEC §36).** The answer equals folding every selected sample at or
  before `state_time`, in replay order, keeping each signal's last.
  `crates/state/tests/properties.rs` proves it against a brute-force scan and against a
  `BY_TIME` query folded that way, with samples dealt among two overlapping segments
  and the write path's buffers, some in both, at random chunk and slice sizes.

### How it reads

Each signal is LATEST over `[state_time - max_age, state_time]`: `norn-query`'s
`Newest`, which is `latest.rs`, with no filter. So everything [above](#one-read) holds:
the tail is read before the snapshot, rows collapse by (signal, time, sequence), and a
vanished file or `SegmentGone` costs a re-read on a fresh snapshot. In detail:

1. The tail's newest buffered time at or before `t`, then `nearest_at_or_before` on the
   index. The latter reads the per-signal summary first, so an absent signal, or a `t`
   after a signal's last sample, costs no walk. A `t` inside a long gap in a signal's
   history still walks back across the gap.
2. Of the two, the later time is a lower bound `s` on the answer: a chunk ending by `t`
   holds a sample at its end, and one straddling `t` holds one from its start.
3. Read `s..=t` (not `s + 1`, because a sample at `s` with a higher sequence can sit in
   another chunk), tail and chunks, and keep the greatest (time, sequence).

There are no checkpoints: [storage.md](storage.md#state-checkpoints) has the
measurements and the decision.

A chunk that fails to read is `DATA_LOSS`, `ERROR_REASON_DATA_CORRUPTED`, with the
same metadata as a query's. It is never reported as a signal with no state.

**Threads.** Above 512 signals an answer reads on up to 8 scoped threads, each with its
own files and charge, joined before it returns. Chunks are per signal, so decoding
dominates and divides cleanly.

**Memory.** An entry is charged to the query budget before it is held, with its name
and any string or bytes payload; each thread also holds one read's rows and its files.

### Pages pin the answer

A sample can arrive between two pages, and no stored sample says when it arrived, so a
later page cannot be recomputed to agree with an earlier one. The first call therefore
computes the whole answer and, if it does not fit one page, **pins** it: the answer
stays in memory, charged to the query budget, and the page token names it. Page size
is capped at 10,000 entries (1,000 when unset) and about 2 MiB.

| Bound | Value | Picked, not measured |
| --- | --- | --- |
| Pinned answers | 16; the oldest goes first | yes |
| Idle time before a pin goes | 60 s since its last page | yes |

A token is bound by a fingerprint to every field but `page_size` and `page_token`
(AIP-158 lets the size change). A token for another request, a finished answer, an
evicted or expired pin, or a daemon that has restarted fails with `INVALID_ARGUMENT`,
`ERROR_REASON_INVALID_TOKEN`: start again from the first page.

## Not implemented

- Checkpoints for StateAt: deliberately, see above.
- `OR`, `NOT` and parentheses in a filter.
- A decoded-chunk cache, and parallel decoding.
- Pushing `MIN`, `MAX` or `COUNT` down into chunk metadata.
- A per-query deadline on the server.
