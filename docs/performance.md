<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Performance

> The benchmark matrix below is implemented, but its full run has not been made, so
> its tables read "to be measured". Below are the requirements from
> [spec.md](project/spec.md), then the matrix, then what was measured earlier, piece
> by piece. Storage and codec numbers are in [storage.md](reference/storage.md), and
> IPC latency in [ipc.md](reference/ipc.md#measured-latency).

## Requirements

- Benchmark model: 16,000 signals at 1 Hz for 365 days, about 504.6 billion
  samples a year; never assume the dataset fits in memory (§15).
- Normal 16,000 samples/s; bursts of 100,000+ samples/s, benchmarked
  separately (§17).
- Benchmarks: IPC and gRPC latency, shared-memory throughput, samples/s,
  batch throughput, disk throughput, (de)compression throughput, query
  latency, StateAt latency, replay throughput, memory and CPU, at 1,000 /
  10,000 / 16,000 / 100,000 signals and 1 / 10 / 100 Hz (§35).
- Observability metrics listed in §34, themselves bounded.

## Note on current defaults

`norn_core::Config::default()` budgets and limits are picked, not measured.
They are documented on `MemoryBudgets` and `Limits` and are to be revisited
from these benchmarks.

## Benchmark matrix

The matrix runs SPEC's workload (§15, §17, §35) against the real `norn`, over real
gRPC on a Unix socket and over shared memory, and reports it with the machine it ran
on. **Its full run has not been made.** Every table in this section reads "to be
measured" until the owner's run fills it in. The sections after it are earlier
one-off measurements, each with its own conditions; none of them is a matrix result.

### Reproducing it

Use a release build on an otherwise idle machine, with `--data-root` on the disk to
be measured:

```sh
just bench-matrix --full --data-root /path/on/the/disk --out benchmarks/results
```

The JSON and Markdown reports are `matrix-full.{json,md}` under `--out`, by default
`target/bench-matrix/`. They are rewritten after every row, so a run that fails
keeps what it measured. A relative path is taken from the workspace root. The other
flags are in `crates/server/benches/matrix/args.rs`:

- `--repeat N` changes the repeats.
- `--only ingest,planes,history` runs some of the groups.
- `--history-signals N` changes the history's signal count.

The profiles are in `crates/server/benches/matrix/profile.rs`:

| Profile | Command | Repeats | Ingest row | History |
| --- | --- | ---: | --- | --- |
| Full, for the owner | `just bench-matrix --full` | 3 | 300 s sustained, 30 s burst, 120 s scale, 30 s planes | 1,000 signals, 1 week at 1 Hz |
| Quick, for CI | `just bench-matrix --quick` | 3 | 2 s each | 4 signals, 1 week at 1 Hz |
| Smoke, to prove every path | `just bench-smoke` | 1 | 1 s each, 20 signals | 2 signals, 2 hours at 1 Hz |

Only the full profile's numbers belong in the tables below. The others keep SPEC's
shape at a scale that says little about capacity.

### Method

- **Isolation.** Every row runs on a fresh daemon, in a new directory under
  `--data-root`, with its own socket in a new directory under `/tmp`. Each is removed
  when the row ends. The harness never connects to another daemon.
- **Configuration.** One configuration for every row, recorded in the report:
  defaults, with `max_signals` raised to fit the largest row, background maintenance
  off so compaction runs only when timed, and shared memory enabled.
- **Ingest.** One `AppendCall`, one stream, `f64` values, open loop, the client on
  the same machine as the daemon. Batches leave on a schedule. A `THROTTLE` delays
  the next one by its `retry_delay`. A refused batch is counted, not sent again, and
  does not pause the schedule, so it lowers the achieved rate.
- **Ack latency.** It runs from handing a batch over to the first response at or
  above the requested level. Each level implies those below it, and a response
  carries the level the batch has reached, so a `RECEIVED` row measures that reply.
- **History.** Load at `PERSISTED`, stop cleanly and time the restart, compact and
  time it, read back on compacted data, load a little more, `kill -9`, and time the
  restart to its first answer. A recovery that reports corruption fails the run.
- **Disk.** Bytes per sample are file sizes under the stream directory, so headers,
  checksums, indexes and trailers count. The metadata log does not.
- **Repeats.** Rows interleave: every row runs once per round. Each cell is the
  median and nearest-rank p95 over the repeats. With three repeats the p95 is the
  largest value.
- **Recorded with every run.** OS, kernel, architecture, CPU, RAM, disk model,
  filesystem, data root, `rustc`, commit, whether the tree was dirty, daemon
  configuration, and every row's scale.

Not covered by the matrix:

- several clients or streams at once
- CPU use (memory is peak RSS, sampled by `ps` every 100 ms)
- rates above 1 Hz per signal, other than through the burst rows
- Windows

### Ingest

| Row | Signals | Offered/s | Levels | Result |
| --- | ---: | ---: | --- | --- |
| Sustained | 16,000 | 16,000 | RECEIVED, BUFFERED, PERSISTED, DURABLE | to be measured |
| Burst 10× | 16,000 | 160,000 | BUFFERED, DURABLE | to be measured |
| Burst 100× | 16,000 | 1,600,000 | BUFFERED, DURABLE | to be measured |
| Scale | 1,000 / 10,000 / 100,000 | same as signals | BUFFERED | to be measured |

Each result is achieved rate, ack p50 and p99, throttled and refused batches, and
peak RSS.

### Socket against shared memory

| Offered/s | Batch | Socket | Shared memory |
| ---: | ---: | --- | --- |
| 1,000 | 100 | to be measured | to be measured |
| 10,000 | 1,000 | to be measured | to be measured |
| 100,000 | 2,000 | to be measured | to be measured |

These rows use 16,000 signals at `BUFFERED`. `ring_granted` records whether the ring
was actually granted; a row that was refused one ran on the socket.

### Reads, compaction and recovery

| Measure | 1 hour | 1 day | 1 week |
| --- | --- | --- | --- |
| Latest, every signal | to be measured | to be measured | to be measured |
| Range, one signal | to be measured | to be measured | to be measured |
| Aggregate, one signal, 60 s windows | to be measured | to be measured | to be measured |

| Measure | Result |
| --- | --- |
| StateAt, one signal | to be measured |
| StateAt, every signal | to be measured |
| Replay at maximum speed | to be measured |
| Compaction time | to be measured |
| Clean restart | to be measured |
| Restart after `kill -9` | to be measured |
| Disk bytes per sample, ingest layout | to be measured |
| Disk bytes per sample, compacted | to be measured |

### The year, extrapolated

The report computes these from the run's medians. 16,000 signals at 1 Hz for 365 days
is 504,576,000,000 samples.

| Figure | Computed as | Result |
| --- | --- | --- |
| Disk a year, compacted | bytes per sample × samples | to be measured |
| Disk a year, ingest layout | bytes per sample × samples | to be measured |
| Highest achieved rate, as a multiple of 16,000/s | highest ingest row ÷ 16,000 | to be measured |
| Time to ingest a year's backlog | samples ÷ highest achieved rate | to be measured |
| Compaction time for a day, and for a year | samples ÷ compaction rate | to be measured |

The assumptions are printed with the figures in every report:

- Bytes per sample and compaction speed are taken as linear in samples, from a far
  smaller data set than a year. Index and metadata overhead grows with segments and
  signals, and counts only as far as the data set had it.
- The values are the harness's: a sawtooth with noise, as `f64`. Real populations
  compress better or worse, per [storage.md](reference/storage.md).
- One stream, no retention, no replicas, and no filesystem overhead beyond file sizes.
- The highest achieved rate is a lower bound unless that row was saturated.
- The client shares the machine's CPUs with the daemon.

### Regression check in CI

The `bench-matrix` job in `.github/workflows/ci.yml` runs `--quick` on Linux. It
compares with `benchmarks/baseline/quick.json` when that file exists, and fails if a
gated median is worse than the baseline by more than 3×. Rates, median latencies,
times and disk bytes are gated. p99s, memory and counts are reported only, because on
a shared runner they are mostly noise. The check also fails if it compares nothing,
if the baseline's schema or profile differs, or if a gated metric stops producing
values. A different OS, architecture or CPU is printed as a note, not a failure.

**No baseline is committed yet, so the job checks for nothing and says so with a
workflow warning.** A baseline is one machine's numbers. It means something only when
it comes from the machine class that compares with it, and only Linux runners run
this job. To make one, run the job, take `matrix-quick.json` from its
`bench-matrix-quick` artifact, and commit it as `benchmarks/baseline/quick.json`. The
file carries the machine and commit it was measured on. To check locally against a
report from the same machine:

```sh
just bench-matrix --quick --baseline path/to/matrix-quick.json
```

`--threshold FACTOR` changes the 3×.

## Earlier one-off measurements

Everything from here to the end of this page was measured piece by piece, before the
matrix, under the conditions each section states. None of it comes from the harness
above.

### At a glance

All figures are from one Apple M3 Max with a local SSD, release builds, client and
daemon on one machine, over gRPC on a Unix socket. Nothing has been measured on Linux
or Windows yet. The sections below have the full tables and methods.

| What                               | Result                   | How it was measured                                                                                         |
| ---------------------------------- | ------------------------ | ----------------------------------------------------------------------------------------------------------- |
| Sustained append, `DURABLE`        | 16,044 samples/s         | `cargo bench -p norn-server --bench append`: 30 s, 16,000 signals, one call; p50 18.11 ms, p99 29.26 ms     |
| Burst append, `DURABLE`            | 120,010 samples/s        | same, 15 s; p50 16.34 ms, p99 22.68 ms. The offered rate was met, so this is not a maximum                  |
| Peak daemon RSS                    | 110 MiB of 512 MiB       | across the four write-path runs, a 21% share of the default budget                                          |
| Startup, 115 M samples             | 88 to 94 ms              | spawn to first answer after a clean stop, warm page cache; 2.6 s without the marker, 1.19 s after `kill -9` |
| Storage, compacted                 | 1.200 bytes/sample       | 57.6 million samples: one hour of 16,000 signals at 1 Hz, in an assumed mix of signal shapes                |
| Storage, before compaction         | 2.195 bytes/sample       | the same hour                                                                                               |
| A year, compacted                  | about 0.61 TB            | **extrapolated** linearly from that one hour, not a measured year; all-noise signals could reach 3.45 TB    |
| Index startup, compacted year      | 32.0 ms                  | 16,000 signals, 8,760 segments, 140.2 million chunks; real index files, chunk bodies left out               |
| Index lookup, one signal, one hour | p50 49.5 µs, p99 6.53 ms | same dataset, empty index cache, single-threaded; locates chunks without reading them                       |
| gRPC round trip                    | p50 31 µs, p99 45–50 µs  | unary health check, 20,000 calls, two runs                                                                  |
| Query, one signal, one hour        | p50 1.41 ms, p99 2.29 ms | 3,600 raw samples; 200 signals at 1 Hz for one day, a scaled-down set                                       |
| Query, one signal, one day         | p50 6.31 ms, p99 6.69 ms | AVERAGE per minute over 86,400 samples; same set                                                            |
| Query, all signals, latest         | p50 7.22 ms, p99 8.12 ms | LATEST of 200 signals; same set. Not measured at 16,000                                                     |
| StateAt, all signals               | p50 83.0 ms, p99 349 ms  | 16,000 signals, random instants, all pages; 15 minutes of data, scaled down                                 |
| StateAt, one signal                | p50 453 µs, p99 791 µs   | same set                                                                                                    |
| Replay, maximum speed              | 1.15 M samples/s         | 14.4 million samples to one subscriber, three runs                                                          |
| Export, Parquet                    | 2.17 M samples/s         | `nornctl export`, 1 million samples, median of three; 1.66 bytes/sample                                     |
| Import, Parquet                    | 365 k samples/s          | `nornctl import` into a fresh daemon at `PERSISTED`, same data, median of three                             |

Caveats:

- **Year figures are extrapolated** from short runs with an assumed signal mix.
  Measure your own data before sizing disks.
- **Crash tests cover process death** (`kill -9`), not power loss. `DURABLE` is
  designed to survive power loss, and on macOS uses `F_FULLFSYNC`, but no test has cut
  the power.
- Performance is measured on macOS only. Linux and Windows are tested in CI, not
  measured.
- Several results use scaled-down datasets, as noted. The specification's full matrix
  (1,000 to 100,000 signals, 1 to 100 Hz) has not been run.

## Write path

`cargo bench -p norn-server --bench append`, release build, Apple M3 Max, a local
SSD 97% full, with client and daemon on one machine. It runs the real `norn` binary
with default configuration (512 MiB total, 64 MiB ingest, 32 MiB IPC), and the
real gRPC path over a Unix socket: one `AppendSamples` call, 16,000 signals in one
stream, `f64` values, open-loop. Ack latency runs from building the batch to the
response at the requested level. The daemon lives through all four rows in order,
so RSS accumulates and each row's peak is the peak so far. Its data, about 1 GiB,
is deleted afterwards. As implemented, with the temporal index wired in:

| Load | Batch | Level | Offered/s | Achieved/s | Ack p50 | Ack p99 | Throttled | Rejected | Peak RSS |
| --- | ---: | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| sustained, 30 s | 1,600 | BUFFERED | 16,000 | 16,043 | 4.10 ms | 14.49 ms | 0 | 0 | 63 MiB |
| sustained, 30 s | 1,600 | DURABLE | 16,000 | 16,044 | 18.11 ms | 29.26 ms | 0 | 0 | 86 MiB |
| burst, 15 s | 2,400 | BUFFERED | 120,000 | 120,105 | 3.73 ms | 10.52 ms | 0 | 0 | 100 MiB |
| burst, 15 s | 2,400 | DURABLE | 120,000 | 120,010 | 16.34 ms | 22.68 ms | 0 | 0 | 110 MiB |

- Peak RSS stayed at 110 MiB against the 512 MiB total, a 21% share.
- The offered rate was met, so these are not saturation numbers: the maximum rate
  is unmeasured. A run without the index read 905 µs and 6.21 ms for the first row,
  and the other rows matched. Three runs in all; why the first row differs was
  not investigated.
- `DURABLE` includes the 5 ms group-commit wait; `fsync` on macOS is `F_FULLFSYNC`,
  which took about 5 ms in metadata tests. Other platforms are unmeasured.
- 16,000 signals at these rates is 1.5 to 7.5 samples per signal per second over
  the whole run, so this says little about chunk size or compression. Chunks here
  are short by design: `max_chunk_age` or half the stream cap, whichever is first.
- Measured on one call and one stream. Many clients are unmeasured.
- Backpressure is proven by tests (`crates/runtime/tests/backpressure.rs`), not by
  this benchmark, which never reached the high-water mark.

## Temporal index

`cargo bench -p norn-index --bench year`, release build, Apple M3 Max, a local
SSD 97% full. Default budget of 64 MiB; rebuilds on 4 threads. [storage.md](reference/storage.md)
"Temporal index" describes the design.

**What is real and what is not.** Segment files are real and pass the real
reader, but hold a header, a directory and a trailer and no chunk bodies: the
index never reads a body, and 0.61 TB of them would measure nothing else.
Directories use [storage.md](reference/storage.md)'s measured densities.

| Layout | Density | Scale |
| --- | --- | --- |
| compacted | 1 chunk per signal per hour, 1 segment an hour | **measured at full scale**: 16,000 signals, 8,760 segments, 140.2 million chunks |
| ingest | 23 chunks per signal per hour, 4 segments an hour | **one week measured**: 672 segments, 61.8 million chunks; a year would be 141 GB of directories, more than the machine had free, so the year column is a linear extrapolation |

### Size and startup

| Measure | Compacted year (measured) | Ingest week (measured) | Ingest year (extrapolated × 52.1) |
| --- | ---: | ---: | ---: |
| full rebuild | 4.52 s | 1.51 s | 79 s |
| startup load | 32.0 ms | 2.6 ms | 136 ms |
| in memory after load | 0.67 MiB | 0.05 MiB | 2.8 MiB |
| summary on disk | 0.60 MiB | 0.05 MiB | 2.5 MiB |
| sidecars on disk | 2.25 GiB | 1.28 GiB | 66.7 GiB |
| bytes per chunk on disk | 17.2 | 22.2 | 22.2 |

The compacted sidecars are 0.37% of the year's projected 0.61 TB. Under load the
page cache fills what the budget leaves: both layouts ended at exactly 64.00
accounted MiB and never passed it. That figure is an estimate of what the cache
holds. Process RSS was 118 MiB and 160 MiB, including the allocator and the
benchmark itself; nothing here separates the two.

**The rebuild ran on a warm OS cache**, with the files just written. A cold
rebuild must read 6.2 GB of directories (compacted) from the disk and was not
measured; expect it to be limited by read throughput instead.

### Lookup latency

Each row starts from an empty index cache, after a `sync`, with random signals
and times. Lookups locate chunks only; reading them is the query piece's cost. No
lookup opened a segment file: the benchmark asserts it.

Compacted year, measured:

| Lookup | Runs | Chunks each | p50 | p99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| one signal, one hour | 1,000 | 1 | 49.5 µs | 6.53 ms | 11.32 ms |
| one signal, one month (720 h) | 200 | 720 | 20.97 ms | 157.84 ms | 882.12 ms |
| all signals, one minute | 200 | 16,000 | 3.76 ms | 5.01 ms | 5.67 ms |
| nearest at or before | 1,000 | 1 | 47.6 µs | 248.0 µs | 946.6 µs |
| nearest, signal absent everywhere | 20 | 0 | 390.74 ms | 482.31 ms | 482.31 ms |

Ingest layout, one week measured. **The "one month" row covers 168 hours**,
because the dataset does:

| Lookup | Runs | Chunks each | p50 | p99 | Max |
| --- | ---: | ---: | ---: | ---: | ---: |
| one signal, one hour | 1,000 | 23 | 105.7 µs | 318.9 µs | 565.4 µs |
| one signal, 168 hours | 200 | 3,867 | 15.08 ms | 28.58 ms | 28.97 ms |
| all signals, one minute | 200 | 27,940 | 29.78 ms | 39.79 ms | 41.84 ms |
| nearest at or before | 1,000 | 1 | 40.9 µs | 98.3 µs | 135.2 µs |
| nearest, signal absent everywhere | 20 | 0 | 15.04 ms | 28.84 ms | 28.84 ms |

Reading these:

- **All-signals-one-minute returns far more than a minute's chunks.** Chunks are
  not clipped: a compacted chunk spans an hour, so a minute overlaps all 16,000,
  and the reader discards the rest.
- **Tail latency is first-touch cost, most likely physical reads.** The slow runs
  of a one-month lookup fall among the first few, about 2 ms per read, then fade
  as sidecars are touched, with no repairs counted. `sync` before the lookups did
  not remove them, which rules out writeback of the rebuild. That they are disk
  misses is inferred from this pattern, not observed directly. Run-to-run, the
  one-month p99 ranged from 158 ms to 1.5 s, and one cold run took 2.8 s; a cold
  month is about 1,440 small reads, one head and one page per segment.
- **The nearest-absent rows above predate the per-signal summary**, and so does the
  391 ms a silent signal cost. With it ([storage.md](reference/storage.md#the-per-signal-summary)),
  the lookup does not touch a segment: re-measured at a month rather than the year,
  16,000 signals × 720 hours (`NORN_BENCH_HOURS=720`, to keep the disk free), p50
  0.2 µs, p99 0.8 µs, and that cost does not depend on the number of segments. A
  rebuild now also fills the summary: 1.86 s for that month.
- These are single-threaded, on a machine doing nothing else.

## Startup

`cargo bench -p norn-server --bench startup`, release build, Apple M3 Max, a local
SSD. The real `norn` binary, timed from spawn to its first `GetServer` answer, over
115.2 million samples: 16,000 signals × 1 Hz × 2 hours, 31 sealed segments, in ingest
layout. Three runs of each; the last row is one run.

| Start | Time | Segments read and checked | Vouched for |
| --- | ---: | ---: | ---: |
| after a clean stop, marker present | 88 to 94 ms | 0 | 31 |
| marker removed: every chunk checked, as every start did before the marker | 2.59 to 2.67 s | 31 | 0 |
| after `kill -9`, with 60 s of new samples in the journal | 1.19 s | 1 | 31 |

What these do and do not show:

- **The page cache was warm**: the data had just been written. The 2.6 s is CRC and
  directory work, not disk. A cold start without the marker must read every byte, so
  it is slower by the disk; that was not measured. The marked start reads almost
  nothing, so it should change much less, also unmeasured.
- The "before" row is the new binary with the marker removed, not the old binary. It
  takes the same path the old code did, with `reconcile` included.
- The marked start's cost follows signals, not history: one chunk per signal for the
  duplicate state, plus one `stat` per segment. It was measured at this one size.
- **Extrapolated, not measured:** linear from these two hours, a year without the
  marker would take about 3.2 hours warm, and it is what `kill -9` avoids too, since
  only the unvouched part is read. A crash still reads every directory, which this
  benchmark's 31 segments do not stress.
- Compaction's and retention's throughput in a live daemon was not benchmarked here.
  [storage.md](reference/storage.md#measurements) has compaction's own speed.

## Provisioning

A device adapter creates its whole catalogue at start: about 60,000 signals for a
network of 1,000 devices. Created one at a time, that took long
enough that readings arriving meanwhile were dropped, so the registry got two
changes (`crates/metadata/src/registry/commit.rs`, `batch.rs`):

- **Group commit that wakes everyone.** One committer leads an fsync with no lock
  held; when it returns, every waiter wakes at once and returns if its record is
  covered. Before, waiters were released one at a time behind a mutex held across
  the fsync, and throughput stopped rising past 16 concurrent creates.
- **Batch creates.** `BatchCreateEntities` and `BatchCreateSignals` (protocol 1.6)
  make up to 1,000 resources one log write and one fsync.

`cargo bench -p norn-server --bench provision` starts a fresh release `norn` per
scenario and creates 1,000 entities and 63,000 signals over real gRPC on a Unix
socket, one resource per call over 8 to 64 connections, then in batches of 1,000
and 500. **Target, not yet measured at full scale:** the table below is the smoke
run `cargo test --all-targets` makes (4 entities, 100 signals, debug build, macOS),
which shows only that every scenario works end to end:

```text
| Method | Connections | Entities | Signals | Signals/s | Total |
| --- | ---: | ---: | ---: | ---: | ---: |
| Create, one per call | 8 | 0.01 s | 0.13 s | 746 | 0.14 s |
| Create, one per call | 16 | 0.01 s | 0.08 s | 1220 | 0.09 s |
| Create, one per call | 32 | 0.01 s | 0.08 s | 1235 | 0.09 s |
| Create, one per call | 64 | 0.01 s | 0.08 s | 1233 | 0.09 s |
| BatchCreate, 1000 per call | 1 | 0.00 s | 0.01 s | 10231 | 0.01 s |
| BatchCreate, 1000 per call | 4 | 0.00 s | 0.01 s | 10523 | 0.01 s |
| BatchCreate, 500 per call | 8 | 0.00 s | 0.01 s | 10352 | 0.01 s |
```

## Query

`cargo bench -p norn-server --bench query`, release build, Apple M3 Max, a local SSD
98% full, client and daemon on one machine. It runs the real `norn` binary with
default configuration and real gRPC over a Unix socket, one client, closed-loop: each
query fully read before the next. Measured on 2026-11-02, one run.

**Scaled down for the disk.** 200 signals × 1 Hz × 1 day, 17.28 million samples, 32
MiB on disk, deleted afterwards; not 16,000 signals. Loaded at `PERSISTED`, so ingest
layout, not compacted. Values are a drift with 0.01 of noise. A wait past
`max_chunk_age` first, so these read chunks, not buffers. The "one day" is the whole
data set.

| Query | Rounds | Results | p50 | p99 | Samples/s | RSS after |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| one signal, one hour, raw | 500 | 3,600 | 1.41 ms | 2.29 ms | 2.45 M | 86 MiB |
| one signal, one day, AVERAGE per minute | 100 | 1,440 | 6.31 ms | 6.69 ms | 13.6 M | 369 MiB |
| all 200 signals, LATEST | 100 | 200 | 7.22 ms | 8.12 ms | 27,444 | 369 MiB |
| all 200 signals, one hour, by time | 5 | 720,000 | 280.66 ms | 280.73 ms | 2.56 M | 293 MiB |

Samples/s is samples returned or reduced, over wall time. For LATEST, each result
counts one; a query there takes about 36 µs a signal.

- **The RSS is the allocator, not held data.** The daemon held 86 MiB before queries
  and peaked at 381 MiB. `vmmap` at the end showed 12.9 MiB allocated in the default
  malloc zone and 203.8 MiB resident in "Malloc Large (empty)": freed regions macOS
  keeps cached. So the query budget held, but RSS here is not bounded by it, and a
  caller sizing a machine should know that. Linux is unmeasured.
- The raw row is dominated by encoding and the gRPC round trip, since 3,600 samples
  decode in microseconds; the p50 is about 400 ns a sample end to end.
- Compacted data, a year, many clients, cold caches and 16,000 signals are all
  unmeasured. By-time cost grows with how many chunks a slice overlaps: see
  [query.md](reference/query.md#bounded-memory).
- `crates/query/tests` prove the bounds with a counting budget; this benchmark did not
  drive the budget to its limit.

## State at an instant

`cargo bench -p norn-server --bench state`, release build, Apple M3 Max, a local SSD,
client and daemon on one machine. The real `norn` binary, default configuration, real
gRPC over a Unix socket, one client, closed-loop: every page of each answer read
before the next call. Measured on 2026-11-02, one run per scale.

**Scaled down for the disk.** Two sets, each deleted afterwards: 2,000 signals × 1 Hz
× 1 hour (7.2 million samples, 19 MiB), and 16,000 signals × 1 Hz × 15 minutes (14.4
million samples, 102 MiB). Not a day or a year, and not compacted: ingest layout,
loaded at `PERSISTED`. A wait past `max_chunk_age` first, so answers come from chunks.
Instants are spread over the set the same way every run.

| Measurement | 2,000 signals, 1 h: p50 | p99 | 16,000 signals, 15 min: p50 | p99 |
| --- | ---: | ---: | ---: | ---: |
| StateAt, one signal, random T | 170 µs | 240 µs | 453 µs | 791 µs |
| StateAt, all signals, random T | 24.9 ms | 40.4 ms | 83.0 ms | 349 ms |
| StateAt, all signals, after the last sample | 22.6 ms | 23.9 ms | 200 ms | 246 ms |
| Scrubbing, all signals, 5 s steps | 23.3 ms | 24.6 ms | 88.8 ms | 286 ms |
| Scrubbing, one signal, 5 s steps | 125 µs | 160 µs | 364 µs | 449 µs |
| Checkpoint stand-in, every 60 s | 55.8 ms | 79.3 ms | 95.9 ms | 1.39 s |
| Checkpoint stand-in, every 300 s | 80.9 ms | 88.7 ms | 251 ms | 3.31 s |
| Checkpoint stand-in, every 900 s | 91.4 ms | 150 ms | 416 ms | 1.70 s |
| Checkpoint stand-in, every 3600 s | 162 ms | 384 ms | 416 ms | 1.69 s |

Rounds: 500 for one signal at random, 100 for all signals, 200 scrubbing steps, 50
stand-ins at 60 s and 300 s, 10 at 900 s and 3,600 s. Daemon RSS after: 90 MiB and 165
MiB.

- **Before threading**, all 2,000 signals took 72.2 ms p50 and 90.7 ms p99 at a random
  T; reading on up to 8 threads above 512 signals brought that to 24.9 ms.
- **The checkpoint stand-in** is what a checkpoint would still have to do after loading
  itself, taken as free: read every sample from the checkpoint to T, keeping each
  signal's last (`LAST` over that span through `QuerySamples`). It costs more than
  reading StateAt directly at every interval, so checkpoints are deferred:
  [storage.md](reference/storage.md#state-checkpoints).
- 16,000 signals ran on the default configuration; its p99s are the query budget and
  the page cache under one client's back-to-back calls, and are unexplained beyond
  that. 16,000 signals over a year, compacted, is unmeasured, and a `T` inside a long
  gap still walks back across it ([storage.md](reference/storage.md#nearest-at-or-before)).
- The 16,000-signal load first hung: one second of samples was a batch over
  `max_batch_samples`, refused for good. The loader now cuts batches by sample.

## Replay

The same benchmark, after StateAt: a session over every signal of the whole set, at
maximum speed, one subscriber reading through `SubscribeEvents` over gRPC on a Unix
socket, timed from `:resume` to `ENDED`. Each run asserts that every sample arrived.

| Set | Samples | Time | Samples/s, three runs |
| --- | ---: | ---: | ---: |
| 2,000 signals × 1 h | 7.2 M | 3.56 s | 2.02 M, 2.02 M, 2.02 M |
| 16,000 signals × 15 min | 14.4 M | 12.5 s | 1.15 M, 1.15 M, 1.15 M |

Daemon RSS after: 347 and 349 MiB, against 90 and 165 MiB before replay ran; as in the
query run above, that is mostly freed memory the macOS allocator keeps, and is unmeasured
on Linux.

**What the first run found.** A session's slice was at first half of its fair share of
`replay_bytes` (2 MiB at the defaults). Replay reads in time order, and every slice
decodes each chunk it overlaps again, so with 16,000 signals each slice of about two
seconds decoded about 16,000 chunks: the 16,000-signal replay ran for more than ten
minutes without finishing, and 2,000 signals managed 262,000 samples/s. A slice may now
take an eighth of `replay_bytes`, as a query's may of `query_bytes`, and the budget
narrows slices when sessions together want more. A decoded-chunk cache would remove the
repeated decoding altogether ([query.md](reference/query.md#bounded-memory)); it is not
built.

Not measured: several sessions at once, a slow subscriber's effect on throughput, paced
replay's timing accuracy (tested with a fake clock, not measured), compacted data.

## Import and export

`tools/interchange/bench.sh 100 10000`, release build, Apple M3 Max, a local SSD 81%
full, three runs. `tools/interchange/generate.py` writes 1,000,000 samples: 100
signals at 1 Hz for 10,000 s, in an assumed mix of 60% noisy doubles, 20% stepping
float setpoints, 10% booleans and 10% slow int32 counters. Not a real deployment. Each run
imports that CSV into a daemon (the fill), then for each format: `nornctl export all`
from it, `nornctl import` into a fresh daemon at `PERSISTED`, and the same import
again (the repeat, which sends nothing). Every daemon and `nornctl` run on one machine
over a Unix socket. Time and peak RSS are the `nornctl` process's, from
`/usr/bin/time -l`, whose wall time has 10 ms resolution. Each cell is the median of
the three runs. The script checks every import read and sent all 1,000,000 samples, and
every repeat sent none.

| Format | File bytes | Bytes/sample | Export samples/s | Export RSS | Import samples/s | Import RSS | Repeat samples/s |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| CSV | 102,209,278 | 102.21 | 1,020,408 | 13 MiB | 346,021 | 12 MiB | 1,136,364 |
| Parquet | 1,655,477 | 1.66 | 2,173,913 | 27 MiB | 364,964 | 16 MiB | 1,428,571 |
| Arrow IPC | 128,254,618 | 128.25 | 2,325,581 | 19 MiB | 574,713 | 23 MiB | 1,538,462 |
| MCAP | 105,710,065 | 105.71 | 1,562,500 | 17 MiB | 354,610 | 15 MiB | 549,451 |

- Ranges over the three runs: CSV export 588 k to 1.39 M/s, the widest; why the
  first run was slowest was not investigated. Imports 250 k to 578 k/s. The
  fill from the generated CSV ran at 293 k to 366 k samples/s, 11 to 12 MiB.
- Import is bounded by the daemon's `PERSISTED` acknowledgement of each batch, one
  in flight. Its 13 to 16 `THROTTLE` responses a run were waited out. Arrow imports
  faster because it decodes cheapest.
- Parquet is 1.66 bytes a sample against Norn's 1.2 compacted and 2.2 at ingest
  ([storage](reference/storage.md)). Arrow and MCAP are uncompressed, because their
  codecs are C, and CSV is text.
- A repeat import reads everything and sends nothing. Its cost is the lookups for
  samples the daemon may hold. MCAP's is lowest because its time order touches many
  signals per batch, so a lookup spans all signals.
- Client RSS stays flat with size: `crates/cli/tests/exchange_memory.rs` asserts it
  between 100,000 and 400,000 samples.

Not measured: 16,000 signals, files beyond 1 million samples, Linux and Windows.
