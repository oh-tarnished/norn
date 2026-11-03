# Storage

Implemented in `crates/storage` (`norn-storage`, SPEC §12, §13, §20, §40, §42,
§43) and `crates/codec` (`norn-codec`, [Encoding](#encoding)). What storage
does not do yet is listed under [Not implemented](#not-implemented).

## Hierarchy

Database → Stream → Segment → Chunk → Block → Sample.

```text
<database>/streams/<stream id, 16 hex digits>/
    0000000000000000-g0000.nseg     segment: sequence 0, generation 0
    0000000000000001-g0000.nseg
    0000000000000000-g0001.nseg     compaction output covering 0..=1
    quarantine/                     bytes recovery moved aside
    journal/                        the write-ahead journal: more segments
    CLEAN                           segments a clean shutdown vouched for
    LOCK                            held by the stream's one writer
```

A **segment** is one file: append-only while open, immutable once sealed. A
`StreamWriter` rolls to the next segment when one reaches a limit from
`StorageOptions`, so no file grows without bound. A **chunk** holds one
signal's samples for a time range, in append order, and is encoded,
compressed and checksummed independently of every other chunk. A **block** is
one column of a chunk, as a `norn-codec` block in its compression envelope.

## Segment file

All integers are little-endian. `tests/format.rs` pins these offsets.

### Header (48 bytes)

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `NORNSEG\0` |
| 8 | 2 | `format_version`, 1: this container layout |
| 10 | 2 | `schema_version`, 1: the chunk's columns and value-kind tags |
| 12 | 1 | `codec_version`: `norn_codec::CODEC_VERSION` when written |
| 13 | 1 | flags, zero |
| 14 | 2 | `generation`: 0 from ingest, +1 per compaction |
| 16 | 8 | stream id |
| 24 | 8 | `first_seq` |
| 32 | 8 | `last_seq`: equals `first_seq` unless compaction merged segments |
| 40 | 4 | reserved, zero |
| 44 | 4 | CRC-32C of bytes 0..44 |

A reader checks the magic, then the CRC, then the versions, so a bit flip is
reported as corruption and only an intact newer file as
`UnsupportedVersion`. A file from a newer Norn is refused and never modified.

### Chunk frame (48-byte header, then the body)

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 4 | magic `NCHK`: the recovery sync mark |
| 4 | 1 | frame version, 1 |
| 5 | 1 | value kind (`ValueKind::tag`) |
| 6 | 2 | flags, zero (reserved for multi-signal groups) |
| 8 | 8 | signal id |
| 16 | 8 | earliest timestamp, ns |
| 24 | 8 | latest timestamp, ns |
| 32 | 4 | sample count |
| 36 | 4 | body length |
| 40 | 4 | CRC-32C of the body |
| 44 | 4 | CRC-32C of bytes 0..44 |

The body is four sections in fixed order, each a `u32` length followed by a
compression envelope around a codec block:

| Section | Codec family | Notes |
| --- | --- | --- |
| timestamps | `timestamp` | nanoseconds |
| values | by kind | `bool`: boolean. `i32`, `i64`, `u32`: integer, widened. `u64`: integer, bit-reinterpreted. `f32`, `f64`: float, bit-exact. `string`, `bytes`: bytes (UTF-8 re-checked on read). `enum`: enumeration |
| quality | `enumeration` | `Quality::code`, an almost constant column |
| sequence | `integer` | bit-reinterpreted `u64` |

### Directory and trailer (written by sealing)

| Part | Size | Field |
| --- | ---: | --- |
| directory | 4 | magic `NDIR` |
| | 4 | entry count |
| | 44 × n | entries: offset `u64`, body length `u32`, signal `u64`, earliest and latest timestamp `i64`, sample count `u32`, value kind `u8`, flags `u8`, reserved `u16` |
| | 4 | CRC-32C of the directory so far |
| trailer | 8 | directory offset |
| | 8 | seal marker `NORNEND\0` |

The trailer ends the file and is written after the directory, so a crash while
sealing leaves no valid seal: the seal is all-or-nothing without a second file.
A reader also requires the directory to end exactly where the trailer begins.
Chunks are fsynced *before* the seal is written, so a power loss cannot leave
a valid directory over chunks that never reached the disk.

## Checksums

**CRC-32C (Castagnoli), implemented in the crate** (`src/crc32c.rs`:
slicing-by-8 tables built at compile time, no `unsafe`, no dependency).

- A CRC is designed for error detection. CRC-32C detects every burst up to 32
  bits and every odd number of flipped bits. xxHash64 promises nothing about
  any error pattern; its extra bits only lower the random-collision rate from
  2^-32 to 2^-64 per region.
- It is what ext4, Btrfs, iSCSI, LevelDB and RocksDB use for this job, and is
  checkable against published vectors (`crc32c("123456789") = 0xE3069283`).
- Measured at 1,382 MiB/s on the benchmark machine, about 100× the bytes the
  write path below produces. CPU instructions (SSE4.2, ARMv8) would be faster but need
  `unsafe` intrinsics or a dependency; not needed at this rate.

Every byte of a sealed segment is covered by a CRC or a marker:
`tests/corruption.rs` flips each byte of a segment in turn and requires a typed
report every time.

## Write side

`SegmentWriter` appends frames sequentially through a bounded buffer.
Durability is explicit, for the write path to map onto SPEC §19:

| Call | After it returns, the bytes survive |
| --- | --- |
| `append` | nothing: they may still be in the writer's buffer |
| `flush` | a process crash (they are in the OS) |
| `sync` | a power loss: `fsync` of the file, and of its directory the first time |
| `seal` | a power loss, and the segment is immutable and has its directory |

A failed write or fsync poisons the writer: a failed fsync leaves the page
state unknown, so it refuses further work and recovery assesses the file.
`atomic_write` updates metadata files (temp, fsync, rename, fsync directory).
On Windows the directory fsync is a documented no-op: `std` cannot open a
directory handle, and NTFS journals metadata.

### Bounds

`StorageOptions::from_config`. The roll limits are `Config::storage()`; none of
these defaults was measured.

| Option | Default | From |
| --- | ---: | --- |
| `max_segment_bytes` | 256 MiB | `[storage] max_segment_bytes` |
| `max_segment_span_nanos` | 1 hour, of sample time | `[storage] max_segment_age_ms`; storage reads no clock |
| `max_chunks_per_segment` | 95,325 | `index_bytes / 16` of directory entries |
| `max_chunk_samples` | 86,400 | picked; at most `MAX_BLOCK_VALUES` |
| `max_chunk_bytes` | 16 MiB | `chunk_cache_bytes / 8`, at most 64 MiB |
| `write_buffer_bytes` | 1 MiB | `ingest_bytes / 64`, 64 KiB to 8 MiB |

A limit is checked before the next append, so a segment can pass a size limit
by one chunk. Recorded lengths are checked against these bounds before
anything is allocated.

## Read side

`SegmentReader::open` reads the header and, if sealed, the directory; it
decodes nothing. An unsealed segment is listed by walking frame headers, reading
48 bytes per chunk. `select` filters by signal and time range and reads nothing;
`read_chunk` reads one frame, verifies both CRCs and decodes it; `samples`
streams with at most one decoded chunk in memory. `ChunkFile` is the same read
without the directory, for a caller that already holds `ChunkMeta`s from the
temporal index: it reads the header, then only the frames asked for. `tests/partial_read.rs` counts
the bytes: a read costs the header, trailer and directory once, then exactly the
frames selected. A corrupt chunk fails only itself: the iterator yields one
`Err` and continues.

Reads are `seek` + `read`. Memory mapping needs `unsafe`, which this crate
forbids; it stays a later option, to be justified by measurement (SPEC §1).

## Recovery

`recover_stream(dir, opts, mode)` runs at startup, before any writer opens the
stream, and returns a `RecoveryReport` of typed events. `ReportOnly` finds the
same things and changes nothing. Repeating it is safe.

| Finding | Event | Repair |
| --- | --- | --- |
| Header torn or corrupt | `SegmentQuarantined` | file moved to `quarantine/` whole |
| Newer format version | `Err(UnsupportedVersion)` | none; file untouched |
| Sealed, chunk fails its CRC | `Corruption`, `LeftInPlace` | none; reads keep failing with a typed error; reported on every run |
| Unsealed, clean | `Sealed` | directory written over the chunks |
| Unsealed or bad seal, torn tail | `Corruption { TornTail }` | tail copied to quarantine, truncated, sealed |
| Bad directory or trailer | `Corruption { Directory }` | old bytes quarantined, directory rebuilt from frames |
| Unsealed, damaged frame mid-file | `Corruption` with `Quarantined` | copied to quarantine; left out of the new directory; scan resumes after it |
| `*.nseg.tmp` | `UncommittedRemoved` | removed; the compaction's inputs are intact |
| Input of a committed compaction | `SupersededRemoved` | removed, only if the output verified clean in this run |

**Never silently discard (SPEC §20).** Bytes are copied to quarantine and
fsynced before any cut; quarantine files are never overwritten, even if a
sequence number recurs. The only deletions are the last two rows, whose samples
provably exist elsewhere.

**Resynchronising.** A frame with a valid header but a bad body is damage of
exactly its recorded length, because the header CRC vouches for the length.
Bytes that are not a valid header are damage up to the next `NCHK` whose header
CRC passes; with no such frame, they run to end of file and are the torn tail.

**Results.** `tests/crash.rs` cuts a segment at every byte of its last chunk
and of its seal, and inside its header. At every cut, all complete chunks are
kept, sample for sample; the torn chunk is reported at its exact offset and
length (a cut on a frame boundary tears nothing); the quarantine file holds
exactly the bytes removed; a second run is clean. A segment cut in its seal
rebuilds byte-identically. `tests/corruption.rs` covers every byte flip, a
corrupt body with readable neighbours, a corrupt directory, and damage mid-file
that spares the chunks after it. These tests simulate a crash by cutting and
flipping bytes; no real power cut or fault-injecting file system has been used.

## Write path

Implemented in `norn-runtime` (`ingest`), over `norn-storage` and `norn-index`. The
question it answers: an acknowledgement must come within milliseconds, but a
chunk is only small on disk if it holds minutes of one signal (92 bytes of
framing per chunk, fresh codec state per chunk). Flushing per-signal chunks to
satisfy each acknowledgement would write one-sample chunks at 16,000 signals.

**Decision: a write-ahead journal, built from the segment format itself.** Every
admitted batch is first written to `<stream>/journal/` by an ordinary
`StreamWriter` holding raw batch records, then buffered per signal and encoded
into chunks by size or age. `PERSISTED` is that writer's `flush`; `DURABLE` is
its `sync`. No second file format and no second recovery: a journal segment has
the same frames, CRCs, torn-tail handling and quarantine as any other.

```text
AppendSamples ─ admit ─ bounded queue ─ commit task ──────────► journal/ (flush, then group fsync)
 (validate,     (order fixed,            │                       PERSISTED, DURABLE
  dedup,         BUFFERED)               └ per-signal buffers
  reserve)                                   │ size or age: seal a generation (and roll the journal)
                                             ▼
                                         store task: encode in parallel, append in order, fsync,
                                                     then delete that generation's journal segments
```

**A generation** is every signal's buffer, sealed together. Sealing rolls the
journal, so each generation owns whole journal segments, and those are deleted
only after its chunks are fsynced in the stream's segments. All of a stream's
durable writes are sequential; only encoding is parallel. Budget reservations
are released when a generation is stored, so journal plus buffers never exceed
what admission allowed.

**Group commit.** A batch wanting `DURABLE` waits at most
`[storage] group_commit_interval_ms` (5 ms) for others, then one fsync covers all
of them; batches arriving during that fsync share the next. `DURABLE` is reported
only after `sync_data` has returned. A failed fsync poisons the journal writer:
every waiting batch gets its second response with `error` set, the stream refuses
further batches with `STORAGE_UNAVAILABLE`, and restart runs recovery.

**Memory**, per stream, all reserved from the ingest budget before it is held:
the batch queue (1,024 batches), the signal buffers, and one generation being
stored with one more waiting. The stream cap is three quarters of `ingest_bytes`.
A sample costs `size_of::<Sample>()` (64 bytes) plus any string or bytes payload
while queued, then 40 bytes plus the payload in columns (26, and `Vec` doubling's
average slack), until its generation is stored. A generation seals at half the
stream cap or `max_chunk_age`. The encoded frames being written are not reserved:
they are smaller than the columns they came from and are written 256 chunks per
thread at a time.

**Locking.** `<stream>/LOCK` is held with an OS advisory lock (`File::try_lock`,
`flock` or `LockFileEx`) for as long as a `StreamWriter` on the stream or a
repairing recovery runs; a second one fails with `StorageError::Locked`. The OS
drops the lock when the process dies, however it dies, so a crash needs no stale
lock cleanup; the file's contents are only the holder's process ID, for a human.
`<database>/metadata/LOCK` likewise keeps a second daemon off a whole database.
Both owners explicitly unlock before closing their files on drop. On Unix, a
concurrent process spawn can temporarily inherit the same open file description;
closing only the owner's handle would leave the lock held until that inherited
handle closes. Explicit unlock makes a completed stop immediately restartable.

**The index.** The store thread tells `TemporalIndex` about chunks only after
`writer.sync` has returned, so a locator never points past what the file holds:
`publish_chunks` for the open segment, `segment_sealed` for each rolled or closed
one. `PersistFailed` is logged and ignored, since answers stay correct.
`OverBudget` changed nothing, so the store thread seals the open segment to
reclaim room; if the index still cannot take it, or any other index error occurs,
the chunks stay safe on disk, new batches get `REJECT` with `MEMORY_EXHAUSTED` and
a retry delay, and the store thread retries with `refresh` every second until it
succeeds. `Ingest::compaction_applied` is the one call for anything that runs
`compact`. Until the index has a generation's chunks it stays readable in memory:
[query.md](query.md#freshness-no-gap-no-double) says how a query sees every persisted
sample, exactly once. While the index is behind, a generation stays in that tail with
its memory reserved. A stream whose journal fails keeps its buffers readable, and
reserved, until restart moves them into segments.

### Recovery at startup

The daemon recovers every stream before it binds its services, and
`GetServer` publishes the totals (`Server.recovery`). For each stream:

1. `recover_stream` on the segments, then on the journal: torn tails go to
   quarantine, unsealed segments get sealed.
2. Each journal record is read back. A record that passes its CRC is a whole
   batch; a torn one was never acknowledged at `PERSISTED`. So a batch is
   recovered whole or not at all.
3. A journal sample may already be in a segment, if the crash fell after a
   generation's fsync and before its journal segments were deleted. Those
   chunks are found by signal and time range and compared exactly by (signal,
   time, sequence), in memory bounded by the journal, itself bounded by the
   ingest budget. The rest are written as chunks and fsynced.
4. Only then are the journal segments deleted. A crash during recovery repeats
   it, finding what the last attempt wrote.

The index opens last, over the recovered segments, with the index budget. Its
report is published as `Server.index`. A database whose lists and tails exceed
`index_bytes` refuses to start: raise the budget.

Cost: see [Clean shutdown](#clean-shutdown). A stream with a current marker skips
every step above. Otherwise steps 1 to 3 read only what the marker does not vouch
for, plus every segment's directory.

### Duplicates: exactly what is guaranteed

[protocol.md](protocol.md): a sample equal to a stored one in signal, time and sequence is
counted and not stored again. In bounded memory, per stream:

- **Per-signal high-water mark**, the greatest (time, sequence) admitted, 24
  bytes a signal. Above it: new. Equal to it: duplicate. Recovered at startup
  from the stored chunks and the journal.
- **Recent-sample window**: a 64-bit fingerprint of each of the last 262,144
  samples admitted (about 6 MiB). It is reserved from the ingest budget when the
  stream opens, at most an eighth of the budget, and halved until it fits; below
  256 entries a stream has marks alone. It is consulted only for a sample below
  its signal's mark, to tell a resend from late data. Rebuilt at startup from each
  signal's newest chunks, then the journal.

So:

- **Guaranteed:** a resend of a sample still in the window, or at the high-water
  mark, is counted in `duplicate_count` and not stored. A batch unacknowledged at
  a crash is in the journal, or in its signals' newest chunks if its generation
  was stored, so it is in the rebuilt window as long as those fit the window.
- **Not guaranteed:** a resend older than the window, below the mark. It is
  indistinguishable from late data, and late data is kept (never dropping is the
  stronger rule, SPEC §18), so that sample is stored twice. Equal rows are
  identical in every field, and a reader can collapse them.
- **Known false positive:** a late sample whose fingerprint collides with a
  window entry is counted as a duplicate and not stored: below 2^-46 per late
  sample, and in-order samples never consult the window.

## Compaction

`compact(dir, inputs, opts)` rewrites sealed segments as one: each signal's
chunks are merged, in order, up to `max_chunk_samples`, and re-encoded with
`Compression::ZstdSmallest`. Every sample is preserved; the output's sample count
must equal the inputs' before it can commit. Its memory is one merged chunk, one
input chunk and the directories.

Crash safety is write new, switch, delete old:

1. Write `<name>.nseg.tmp`, seal, fsync. A crash leaves the inputs live.
2. Rename to `<name>.nseg`, fsync the directory. **This is the commit**: the
   output has a higher generation and its header covers the inputs' sequence
   range, so it supersedes them. No separate manifest is involved.
3. Delete the inputs, fsync the directory. A crash leaves superseded files, which
   `live_segments` ignores and recovery removes.

`compact_observed` is the same, with a `CompactionObserver` that sees each
`CompactionPhase` and may cancel before the commit. Merging also stops at half of
`max_chunk_bytes` of string or byte payload, since a merged chunk must stay readable.

It refuses (`CompactionRefused`, nothing changed) an unsealed input, mixed
streams, or inputs omitting a live segment in their range, which would otherwise
be superseded unmerged. A corrupt input aborts before the commit: damage is
never compacted over. Ingest is unaffected, since it writes only newer segments.

## Maintenance

`norn-runtime`'s `maintenance` module runs compaction, retention and verification on
**one worker, one job at a time**. That one choice gives three properties: the CPU
and disk taken from ingest are bounded, two jobs can never touch one stream, and no
job needs a lock against another. Jobs run on tokio's blocking pool. At most 16 wait,
and past that a request fails with `RESOURCE_EXHAUSTED`. A long `verify` delays
compaction, which is the price.

**Choosing a run.** From the index's sealed segments, neighbours by sequence number,
at most `max_segment_bytes` of input spanning at most `max_segment_age`. A run is two
or more segments, or one fresh from ingest. So each compaction lowers the segment
count or raises a generation from zero, and asking again terminates. The open
segment is never an input, because it is not sealed. Segments with known damage split
runs and are skipped.

**Not blocking ingest.** Compaction takes no write-path lock and only reads sealed
files. What it shares is the index's update mutex, for one directory read per
compaction: the store thread can wait that long, and an acknowledgement does not,
since journalling never touches the index.

**Readers.** A snapshot holds the inputs or the output, never both. A reader that
meets a deleted input refreshes and reads the slice again
([concurrency](#concurrency)).

**Memory.** One run's directories, a merged chunk, an input chunk and an encoded
frame, reserved from `chunk_cache_bytes` (which nothing else draws on) and capped at
that budget.

**Failure.** A failed run leaves its inputs live, is logged and counted in
`compaction.failures`, and the worker carries on. Damage in an input marks the
segment skipped. A run too large for one segment is not retried until restart.

**Shutdown.** Cancellation stops a compaction within one chunk, before its commit;
queued jobs end cancelled. The daemon waits for the worker before closing the write
path, so files have stopped moving when the markers are written.

**Operations** live in memory, at most 256, the oldest finished evicted first. A
restart forgets them; `GetOperation` then fails with `NOT_FOUND`. What they did is
durable.

## Retention

`segments_before(dir, cutoff, opts)` lists sealed live segments whose every sample is
before `cutoff`, and `segments_over_size(dir, max_bytes, opts)` the oldest that would
have to go to fit a limit. `remove_segments(dir, paths)` deletes what a caller names.
**The storage crate decides nothing** (SPEC §43). The maintenance worker decides, from
index records, which cost no read:

| Policy | Removes |
| --- | --- |
| `Forever` (the default) | nothing |
| `MaxAge(d)` | segments whose newest sample is before `newest - d` |
| `MaxMonths(n)` | the same, with the cutoff `n` calendar months back in UTC, on the same day or the month's last |
| `MaxBytes(n)` | the oldest segments until sealed bytes fit, buffered samples not counted |

`newest` is the stream's newest sample, buffered ones included, so age never runs
against the wall clock. Unsealed and empty segments are never candidates, and **the
segment with the highest sequence number always stays**. That keeps sequence numbers
from being reused, which is what lets a stale marker never match a new file.

A pass logs at `WARN` first, deletes oldest first (a crash leaves an unfinished pass,
never a hole), calls `refresh(stream)`, adjusts the stream's totals, and counts the
removal for `GetServer`. A dry run stops before the log line and changes nothing.

## Clean shutdown

Reading every chunk at every start costs time proportional to all stored data. A
sealed segment is fsynced before its seal and never changes, so a crash cannot have
touched it. What could be wrong with it is what could be wrong with any byte on a
disk at any time, and startup is the wrong moment to look: `VerifyStream` is the
right one.

`<stream>/CLEAN` is written atomically after the write path closes:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `NORNCLN\0` |
| 8 | 2 + 2 | version, 1; reserved |
| 12 | 8 | stream id |
| 20 | 4 | entry count |
| 24 | 44 × n | sequence `u64`; generation `u16`; reserved `u16`; file length, samples `u64`; earliest, latest `i64` |
| end | 4 | CRC-32C of everything before it |

It says these segments were sealed and intact. That stays true after the daemon
starts again, so it is **not** deleted at start:

| At start | What happens |
| --- | --- |
| Directory holds exactly what the marker lists, no `.tmp`, journal empty | **Clean.** Nothing is read but the marker. Totals come from it, and each signal's duplicate state from one chunk through the index |
| Anything else | Listed segments of the same name and length have only their header and directory read. Everything else is read and checked, as is the journal |
| No marker, or one that fails its CRC | Everything is read and checked |

A vouched segment whose header or directory does not verify gets the full treatment
anyway. A marker is not written, and an old one is removed, when a stream failed, its
index fell behind, samples remain unstored, it holds known damage, or its directory
differs from the index. Finding damage removes the marker at once, so a crash
afterwards still reads the stream and repeats the report. A kill leaves the old marker
in place, which is sound: compaction outputs and new segments are not on it.

Known limits:

- **Bit rot in a vouched segment is not found at start.** It is found by `verify`, a
  query, or a compaction reading it. A start used to find it, at the cost above.
- The duplicate high-water mark comes from one chunk ending at each signal's latest
  time. If chunks of two segments end at that very instant with different sequences,
  it may be the lower one, and a resend of the other is then stored as late data.
- A crash still reads every directory, twice in `reconcile`.

## Measurements

`cargo bench -p norn-storage --bench ingest`, on an Apple M3 Max, one thread, writing
to the system temp directory. 16,000 signals × 1 Hz × 1 hour (57.6 million samples), through
the real writer, compactor and reader.

The population reuses the codec benchmark's shapes (`gen.rs`), an assumed mix:
1/7 of signals each of setpoint, drift `f64`, drift `f32`, noisy analog, on/off,
multi-state and energy counter, each at its own offset into the day. One signal
in five has jittered, gappy timestamps; about 1 sample in 1000 is uncertain.
Ingest chunks are as large as the default 64 MiB ingest budget allows at 26 bytes
per buffered sample: 161 samples, about 2.7 minutes. Each 161-second window is
fsynced. Sizes are file sizes: every header, CRC, directory and trailer counts.

| Layout | Segments | Chunks | Bytes | Bytes/sample | Year at 16,000 × 1 Hz |
| --- | ---: | ---: | ---: | ---: | ---: |
| ingest, zstd 3 | 4 | 368,000 | 126,437,119 | 2.195 | 1.11 TB |
| compacted, smallest of zstd 3–6 | 1 | 16,000 | 69,124,730 | 1.200 | 0.61 TB |

| Step | Seconds | Samples/s |
| --- | ---: | ---: |
| encode and append | 10.62 | 5.42 M |
| fsync (per window) and seal | 0.16 | |
| write total | 10.78 | 5.34 M, 334× the 16,000 samples/s target |
| compaction (four zstd levels per block) | 4.75 | 12.1 M |
| streaming read of every sample | 0.71 | 81 M |

Opening the compacted segment reads 704,084 bytes (16,000 directory entries).

| Shape | Ingest bytes/sample | Compacted bytes/sample |
| --- | ---: | ---: |
| flat setpoint (`f64`) | 0.919 | 0.151 |
| drift 0.1 (`f64`) | 0.988 | 0.201 |
| drift 0.1 (`f32`) | 0.985 | 0.201 |
| noisy analog (`f64`) | 7.317 | 6.831 |
| on/off (`bool`) | 0.891 | 0.150 |
| multi-state (enum) | 0.880 | 0.150 |
| energy counter (`i64`) | 1.419 | 0.632 |

**Year projection** (504.576 billion samples, linear from this hour):

- **0.61 TB compacted**, 1.11 TB if never compacted. A year is about 8,760
  compacted segments of 69 MB.
- The mix decides this, not the format. Full-precision noise is 81% of the
  compacted bytes while being 1/7 of the signals. The projection is 0.12 TB with
  none of it and 3.45 TB if every signal were noise. Real sites should be
  measured before sizing disks.
- Compaction saves 45%. Small chunks pay for their 92 bytes of framing per chunk
  (0.57 bytes/sample at 161 samples, 0.026 at 3,600) and for fresh codec and zstd
  state per chunk. Default budgets also roll segments on chunk count, four an
  hour instead of one.
- Merging into chunks longer than an hour was not measured, and the 365-day
  figure is an extrapolation from one hour, not a year-long run.

## Temporal index

Implemented in `crates/index` (`norn-index`, SPEC §21, stage 7). It answers
*which chunks?* without a data file being opened: signal → chunks, time range
→ chunks, stream → chunks, and timestamp → nearest chunk at or before.
**Entity → signals is not here**: `norn-metadata` owns it, because an entity
belongs to the catalogue rather than to time. Query asks metadata for the
signals, then the index for their chunks. No sample is ever indexed; the
smallest thing the index knows about is a chunk.

### Why three tiers

The target is 16,000 signals for a year, which is 140 million chunks compacted
(8,760 segments) and 3.2 billion if never compacted (35,040 segments), against
a default budget of 64 MiB. Holding one entry per chunk in memory is out by
two orders of magnitude, and reading each segment's own directory on a query
costs 704 KB per segment, or half a gigabyte for one signal over one month.

| Tier | Where | Holds | Cost at the target |
| --- | --- | --- | --- |
| Segment list | memory, always | one 72-byte `SegmentRecord` per live segment, by stream, sorted by time, plus a running maximum of `max_ts` | 80 bytes × segments |
| Chunk detail | sidecar file per segment; pages cached | per-signal directory: chunks sorted by signal, in pages of 128, behind a fence table | one small read per segment touched |
| Segment | the segment file | the data | opened by the reader, never by a lookup |

Overlapping time ranges (late data, compaction) are handled by the running
maximum: segments are sorted by `min_ts`, so a binary search on it gives the
first segment that can reach `from`, and one on `min_ts` the last that can
reach `to`. Each is O(log n), and a range query never scans the list.

### Files

Everything is under `<database>/index/<stream id, 16 hex digits>/`, apart from
the segments, so deleting `index/` is always safe.

```text
<database>/index/<stream>/
    summary.nidx                       the segment list of this stream
    signals.nsig                       the per-signal summary of this stream
    0000000000000000-g0001.ndet        chunk detail of that segment
```

`summary.nidx` (written atomically: temp, fsync, rename, fsync directory):

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `NORNIDX\0` |
| 8 | 2 | version, 1 |
| 10 | 2 | reserved, zero |
| 12 | 4 | record count |
| 16 | 8 | stream id |
| 24 | 72 × n | records: `first_seq`, `last_seq` `u64`; generation `u16`; reserved `u16`; chunks `u32`; samples, file length `u64`; `min_ts`, `max_ts` `i64`; lowest and highest signal `u64` |
| end | 4 | CRC-32C of everything before it |

`*.ndet` (temp, then rename, **not fsynced**: it is derived and checksummed, so
losing one costs a re-read, and 8,760 fsyncs would dominate a rebuild):

| Part | Size | Field |
| --- | ---: | --- |
| head | 8 | magic `NORNDET\0` |
| | 2 + 2 | version, 1; reserved |
| | 4 | page count |
| | 8 + 8 + 2 + 2 | stream; `first_seq`; generation; reserved |
| | 4 + 8 | chunk count; the segment's file length, which ties the sidecar to it |
| fences | 12 × (pages + 1) | first signal `u64`, page offset `u32`; the last is a sentinel holding the end |
| | 4 | CRC-32C of head and fences |
| page | varies | up to 128 entries, then a CRC-32C of them |

An entry is varints, about 18 bytes against the directory's 44: signal delta;
zigzag offset relative to the previous frame's end; body length; zigzag `min_ts`
delta; zigzag `max_ts - min_ts`; sample count; one byte of value kind. Chunks
sort by `(signal, min_ts, offset)`.

### Staying correct

The index is derived. **Segment directories are the only input and
`live_segments` is the authority.** The rule for supersession is the same
predicate, computed by a sort-and-sweep rather than `live_segments`' O(n²),
and a property test holds them equal.

| Situation | What happens | Reported as |
| --- | --- | --- |
| `summary.nidx` missing, torn, bit-flipped, newer version, other stream | discarded; every segment directory is read | `StreamReport::summary` is `Discarded(reason)` |
| A segment on disk not in the summary, or of another length | its directory is read | `read` |
| In the summary but gone from disk | dropped | `dropped` |
| Input of a compaction still on disk | not live, so never answered | |
| Sidecar missing, torn, bit-flipped, or for another file length | answered from the segment's directory and rewritten | `IndexStats::detail_repairs` |
| Unsealed segment at startup | listed by frame headers and served as a tail | `tails` |
| On `refresh`, an unknown file shorter than a header | skipped: a writer created it and has not flushed its header; that writer publishes it | |
| Orphan sidecars and `.tmp` files | removed at startup | `orphans_removed` |

A crash can only leave an old summary, a stray temp file or orphan sidecars, and
all three are in the table. The order is: seal or compact, write the sidecar,
swap the in-memory snapshot, write the summary. Nothing is lost at any cut,
because the only data in these files is a copy.

Startup costs one `read_dir` and one `stat` per segment, plus the summary. It
trusts a record whose name and length match, since a sealed segment is
immutable; a directory recovery rewrote in place keeps its chunks, or it changes
length. Run `recover_stream` first, as with `StreamWriter`.

### Unsealed segments

A sealed segment's detail is on disk. The segment being written has none, so the
writer publishes its `ChunkMeta`s after `flush`, into an in-memory tail, one
per unsealed segment:

```text
writer.append(chunk)   -> Appended { chunk, rolled }
index.segment_sealed(rolled)        when the append rolled a segment
writer.flush() or sync()
index.publish_chunks(stream, segment, &metas)
```

Publishing before the flush would hand readers an offset beyond the end of the
file. A tail is frozen blocks of 256 metas plus one open block, shared between
snapshots, so publishing copies at most one block rather than the whole tail.
Sealing swaps the tail for the sealed record in one step: a chunk never
disappears, and is never answered twice.

### Concurrency

Readers take an immutable `Snapshot` from a pointer guarded by an `RwLock`, held
only to clone an `Arc`. Updates build a new snapshot, doing their I/O outside
any lock, and swap the pointer. A reader therefore holds up nothing but a
pointer copy, whatever it is doing with the snapshot, and a lookup takes one
short mutex for the page cache. An old snapshot pins its segment list and tails
until the last reader drops it; that memory is not counted against the budget.

**What a reader sees during compaction.** A snapshot holds either the inputs or
the output, never both and never neither. The answer changes in one step, and
only in having fewer chunks. But the files move first: `compact` deletes the
inputs before the index can be told, so a plan may yield a locator whose file
has gone, or fail with `SegmentGone`. In both cases the reader:

1. calls `TemporalIndex::refresh(stream)`, which is idempotent and reconciles
   with the directory, and takes a new snapshot;
2. plans again, from the last timestamp it delivered per signal, dropping samples
   it has delivered. Chunk boundaries have changed, so a locator from the old
   plan means nothing in the new one.

### The per-signal summary

`nearest_at_or_before` once walked every segment back from `t` for a signal gone
silent, and every segment there is for one that never existed: 391 ms over a compacted
year. Each stream view now holds a sorted vector of one entry per signal: its earliest
and latest timestamp, and a segment holding the chunk that ends at the latest. 56 bytes
a signal, charged to the index budget, so about 5.6 MiB at `max_signals`.
`Snapshot::last_seen` reads it, and:

| Lookup | Cost |
| --- | --- |
| absent signal, or `t` before its first sample | no read |
| `t` at or after its last sample, however long it has been silent | one segment's head and page |
| `t` before its last sample | the old walk, which still crosses a long gap in the middle of a signal's history |

Publishing a tail or sealing a segment merges its chunks in; a chunk ending at or
after a signal's latest takes it over, which is also what moves a signal off a
compaction's inputs. A publish costs one copy of the vector.

It is derived like the rest. `signals.nsig` is written with `summary.nidx`, atomically,
and tied by a fingerprint to the exact set of sealed segments it describes:

| Offset | Size | Field |
| ---: | ---: | --- |
| 0 | 8 | magic `NORNSIG\0` |
| 8 | 2 + 2 | version, 1; reserved |
| 12 | 4 | CRC-32C of the sorted (`first_seq`, generation, file length) of every sealed segment |
| 16 | 8 | stream id |
| 24 | 4 | entry count |
| 28 | 34 × n | signal `u64`; first, last `i64`; last segment's `first_seq` `u64`, generation `u16` |
| end | 4 | CRC-32C of everything before it |

Missing, damaged, stale, or naming a dead segment: every sidecar is read at startup
and it is rewritten. `refresh` reads only
what is new, and rebuilds only when retention removed the segment a signal was last
seen in. After retention, `first` is a lower bound.

### Plans

`Snapshot::plan` is one iterator for all the lookups: it streams `ChunkLocator`s
in the total order `(min_ts, stream, first_seq, generation, offset)`. It merges
segments lazily: a segment's detail is read only once nothing earlier can come
out, so memory is the chunks of the segments whose time ranges overlap at that
moment, not the size of the answer. `IndexOptions::max_plan_buffered` bounds it,
and passing it is a typed `PlanTooLarge`, not an allocation.

### Nearest at or before

`nearest_at_or_before(signal, t)` returns the chunk maximising `min(max_ts, t)`
among those with `min_ts <= t`, then latest `min_ts`, then the plan's order. When `t`
is at or after the signal's last sample it looks in one segment
([the summary](#the-per-signal-summary)), so among chunks of different segments ending
at that very instant it may return any.
That chunk always holds a sample at or before `t`. With overlapping chunks it
need not hold the *latest* one: a long sparse chunk can straddle `t` while a
short one holds something closer. So StateAt must read the chunk, find the last
sample `s <= t`, and plan `(signal, s + 1..=t)`, which is empty and cheap except
under overlap (plan from `s` itself if two samples may share a time); `tests/nearest.rs` proves that two-step recovers the true sample.

### Memory

`IndexOptions::memory_budget_bytes` is `MemoryBudgets::index_bytes`, passed in
as a value since `norn-index` cannot import `norn-runtime`. Segment lists and
tails are charged first and the page cache gets the remainder, evicting least
recently used. When lists and tails alone would pass the budget, the update or
open fails with `OverBudget` and changes nothing: the write path treats it as
backpressure, and sealing or compacting reclaims the room. Nothing on disk
depends on it.

Measured numbers are in [performance.md](../performance.md).

## State checkpoints

SPEC §24 asks for periodic state checkpoints, at an interval chosen by measuring 1 min,
5 min, 15 min and 1 h. **They are deferred, because measurement says they would be
slower than what exists.**

Every sample is stored and indexed, so `StateAt(T)` needs no checkpoint: each signal's
latest sample at or before T is one lookup in the per-signal summary and one small
read ([query.md](query.md#state-at-an-instant)). A checkpoint at C would replace that
with "load the checkpoint, then read every sample in C..T". The benchmark measured the
second half alone, through the real API, taking the load as free:

| Interval | 2,000 signals, p50 | 16,000 signals, p50 | Direct StateAt, all signals |
| --- | ---: | ---: | ---: |
| 1 min | 55.8 ms | 95.9 ms | 24.9 ms (2,000), 83.0 ms (16,000) |
| 5 min | 80.9 ms | 251 ms | |
| 15 min | 91.4 ms | 416 ms | |
| 1 h | 162 ms | 416 ms | |

At 1 Hz, even a 1-minute interval leaves 60 samples a signal to read, where the direct
read decodes one chunk's worth. Scrubbing at 5-second steps answers in 23.3 ms for
2,000 signals and 88.8 ms for 16,000, fast enough to drag. Full table and method:
[performance.md](../performance.md#state-at-an-instant).

Where checkpoints would pay, and when to revisit:

- **A T inside a long silence.** The index walks back across the gap. A per-signal
  checkpoint would bound that; so would extending the per-signal summary.
- **Compacted years.** Measured on hours, in ingest layout.
- **Many more signals per answer**, where one lookup each stops being cheap.

Nothing in the API names a checkpoint (SPEC §26), so adding them later changes no
client.

## Not implemented

- **State checkpoints.** Deferred by measurement: see above.

- **Multi-signal chunks.** A chunk holds one signal; the frame's flags are
  reserved for groups.
- **Salvage from a quarantined file.** A file with an unusable header is kept
  whole, but no tool reads the chunks inside it.

Known limits of the design:

- Resynchronising after a damaged frame header in an unsealed segment could
  accept a complete, CRC-valid frame stored as the content of a `bytes` value.
- Recovering with a smaller `max_chunk_bytes` than the data was written with
  treats a larger chunk in an unsealed segment as damage: quarantined, not lost,
  but not listed.
- `live_segments` trusts a committed compaction output by its header alone;
  checking it is recovery's job, which keeps the inputs if it is damaged.
- If deleting a compaction's inputs fails, and retention later removes the output
  before a restart, those inputs become live again at the next start: old data
  returns, and the next retention pass removes it.

## Encoding

Implemented in `crates/codec` (`norn-codec`), SPEC §14 and stage 6. The codec
works on primitive slices and returns bytes; it knows nothing of chunks,
files or signals, and the storage layer decides where blocks live.

### Block layout

Every encoded column block is self-describing (SPEC §40):

| Field | Size | Meaning |
| --- | --- | --- |
| `codec_id` | 1 byte | Which encoding; also fixes the column type |
| `codec_version` | 1 byte | Layout version of that encoding; currently 1 for all |
| `count` | varint | Number of values (at most `MAX_BLOCK_VALUES`) |
| payload | rest | Defined by the codec |

A decoder refuses an unknown id (`UnknownCodec`), an unknown version
(`UnsupportedVersion`) and a block of another column type (`WrongColumn`)
rather than misreading it. Ids are never reused: a changed layout bumps
`codec_version`.

Optionally, the block is then wrapped in a **compression envelope**:
`kind: u8` (0 raw, 1 zstd), `level: u8`, `raw_len: varint`, body. A block
that zstd does not shrink is stored raw, so data that is already dense is
never recompressed blindly.

### Codecs

The `encode` function for each column measures its candidates and keeps the
smallest. For `i64` this is an exact size computed without writing the
output; floats are encoded both ways. `encode_with` forces a codec.

| Column | Id | Codec | Layout |
| --- | --- | --- | --- |
| Timestamp / Integer (`i64`) | 0x01 / 0x11 | Regular | start, step (zigzag varints) |
| | 0x02 / 0x12 | Delta | first value, then patched-FOR deltas |
| | 0x03 / 0x13 | Delta-of-delta | first value, first delta, then patched-FOR second deltas |
| | 0x04 / 0x14 | FOR | patched-FOR values |
| | 0x05 / 0x15 | RLE | run count, patched-FOR run values, patched-FOR run lengths |
| Boolean | 0x21 | Bits | one bit per value |
| | 0x22 | RLE | run count, first value, patched-FOR run lengths |
| Enum (`u32`) | 0x31 | Dictionary | distinct codes (first-appearance order), bit-packed indices |
| | 0x32 | Dictionary + RLE | dictionary, then run indices (bit-packed) and run lengths |
| `f64` / `f32` | 0x41 / 0x49 | Gorilla | XOR with the previous value's bits, leading/trailing-zero windows |
| | 0x42 / 0x4a | ALP | exponents `e`, `f`; exceptions (positions and raw bits); nested integer block of digits |
| Bytes | 0x51 | Plain | patched-FOR lengths, then the bytes back to back |
| | 0x52 | Dictionary | distinct values, bit-packed indices |

**Patched FOR** is the building block under every integer layout. Values go
in miniblocks of 128. Each miniblock stores a base, then a flag byte (the bit
width, plus a "patched" bit), then `v - base` packed at that width. Values
outside `[base, base + 2^width)` are exceptions: a count, one-byte positions,
and zigzag varints. The planner trims up to 16 values from either tail of the
sorted miniblock and keeps the cheapest result. This way one timestamp gap or
jittered sample no longer widens the 128 values around it. All arithmetic
wraps, so every `i64` round-trips, including `i64::MIN` and `i64::MAX`.

**ALP** (Afroozeh, Kuffó and Boncz, SIGMOD 2024, scalar variant) is
implemented because it wins on realistic data (see below). It finds `e >= f`
such that `d = round(v * 10^e * 10^-f)` round-trips *bit for bit* through
`d * 10^f * 10^-e`, searching a sample of 256 values. The digits `d` go
through the integer codec. NaN, `-0.0`, infinities and values that fail the
exact check are stored raw as exceptions, so ALP is lossless on every bit
pattern.

### Bounds and failure behaviour

- At most `MAX_BLOCK_VALUES` = 2^20 values per block (an `i64`/`f64` block
  decodes to at most 8 MiB). Byte-string payloads and the envelope's
  `raw_len` are capped at `MAX_BLOCK_BYTES` = 16 MiB. A dictionary block's
  expanded size is checked against that cap before it is copied.
- Every count is checked against these limits *before* anything is
  allocated. The zstd decoder writes into a buffer of exactly `raw_len`
  bytes, and its window is capped at 16 MiB.
- Decoding never panics. Malformed input returns a typed `CodecError`
  carrying the byte offset: `Truncated`, `Corrupt`, `TooLarge`,
  `TrailingBytes`, `Decompress`, and so on. Tests cover every truncation of
  a block for every codec, 40,000 random corruptions, and hostile counts.
- Property tests check `decode(encode(x)) == x`, compared by bit pattern
  for floats, for every codec. Inputs include NaN payloads, `±0.0`,
  infinities, subnormals, `i64` extremes, and empty and single-element
  blocks.

### Block compression: the pure-Rust zstd finding

Per the owner's decision (SPEC §14), zstd comes from a pure-Rust encoder at
levels 3–6. Candidates were read and measured on 2026-11-01:

| Crate | Encoder at levels 3–6 | Notes |
| --- | --- | --- |
| `ruzstd` 0.9.1 (76M downloads) | **No.** `Default`/`Better`/`Best` hit `unimplemented!()` in `frame_compressor.rs` and panic (reproduced) | Only `Fastest` (about level 1) encodes |
| `oxiarc-zstd` 0.4.2 | **Not really.** `LevelConfig::for_level` maps 4..=6 to one hash-chain config that differs only in search depth; output is 12–26% larger than C zstd on text | No `unsafe` |
| `zstd-pure-rs` 0.2.0 | Yes; a line-by-line port of C zstd, matching C sizes | Licence BSD-3-Clause OR GPL-2.0; 513 `unsafe` sites; C-style `ZSTD_*` API |
| **`structured-zstd` 0.2.6** | **Yes.** Its level table mirrors upstream `clevels.h`: 3–4 dfast, 5 greedy, 6 lazy (and greedy/lazy/lazy2/btlazy2 for sources up to 16 KiB) | Apache-2.0; fork of `ruzstd`; no transitive dependencies with default features off |

Measured against the reference C `zstd` 1.5.7 CLI (compressed bytes; every
pure-Rust frame was decoded by `zstd -d` and matched the input):

| Input | Level | C zstd | structured-zstd | oxiarc-zstd |
| --- | --- | ---: | ---: | ---: |
| 4.2 MB Rust source | 3 | 1,045,876 | 1,045,245 | 1,167,907 |
| | 4 | 1,038,508 | 1,038,329 | 1,177,766 |
| | 5 | 972,115 | 971,620 | 1,176,345 |
| | 6 | 933,449 | 933,160 | 1,175,395 |
| 86,400 drift `f64`, raw | 3 / 6 | 17,250 / 14,688 | 17,204 / 14,551 | 17,415 / 16,952 |

**Chosen: `structured-zstd` =0.2.6**, with default features off and only
`std` enabled. It is the only candidate that genuinely implements distinct
levels 3–6, with permissive licensing and an idiomatic API. Its frames are
standard: `tests/zstd_interop.rs` decodes our frames at every level with the
C CLI, and decodes C-produced frames (levels 3, 6 and 19) through our
envelope. Set `NORN_REQUIRE_ZSTD_CLI=1` to make those tests fail when the CLI
is missing.

It declares `rust-version = 1.92` in every release. Norn builds with
Rust 1.99.1, so that dependency requirement is satisfied.

Open points for the owner:

- The crate is young (first release March 2026, about 89k downloads).
- It contains `unsafe` internally (about 1,275 sites, mostly SIMD kernels and
  copy loops). `norn-codec` itself is `#![forbid(unsafe_code)]`.

### Benchmarks

Run with `cargo bench -p norn-codec --bench shapes`, on an Apple M3 Max with
release build. Each shape is one day at 1 Hz (86,400 samples), generated
deterministically. MB/s is over in-memory value bytes: 8 per `i64`/`f64`, 4
per `f32`/`u32`, 1 per `bool`. Raw size is therefore 8, 4 or 1 bytes per
sample.

Measured-best encoding, then zstd (bytes/sample):

| Shape | Chosen codec | Encoded | +zstd 3 | +zstd 4 | +zstd 5 | +zstd 6 | zstd 3 on plain values |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |
| flat setpoint (f64) | Float64Alp | 0.0003 | 0.0003 (raw) | 0.0003 | 0.0003 | 0.0003 | 0.0012 |
| temperature drift 0.1 (f64) | Float64Alp | 0.0549 | 0.0383 | 0.0385 | 0.0384 | 0.0384 | 0.0882 |
| temperature drift 0.1 (f32) | Float32Alp | 0.0549 | 0.0383 | 0.0385 | 0.0384 | 0.0384 | 0.0739 |
| noisy analog (f64) | Float64Alp | 6.8299 | 6.7013 | 6.7010 | 6.6896 | 6.6885 | 7.2390 |
| on/off, rare toggles (bool) | BooleanRle | 0.0004 | 0.0004 (raw) | 0.0004 | 0.0004 | 0.0004 | 0.0008 |
| multi-state (enum) | EnumDictRle | 0.0007 | 0.0008 (raw) | 0.0008 | 0.0008 | 0.0008 | 0.0021 |
| energy counter (i64) | IntegerDelta | 0.5158 | 0.4644 | 0.4644 | 0.4644 | 0.4644 | 1.0302 |
| 1 Hz timestamps, regular | TimestampRegular | 0.0002 | 0.0003 (raw) | 0.0003 | 0.0003 | 0.0003 | 3.4357 |
| 1 Hz timestamps, jitter + gaps | TimestampDelta | 0.9789 | 0.5222 | 0.5220 | 0.5132 | 0.5113 | 3.6151 |

"(raw)" marks blocks that zstd could not shrink. The envelope then stores
them raw, and the extra 0.0001 is the envelope's own 3–4 bytes.

Every codec per shape (bytes/sample; encode and decode MB/s):

| Shape | Codec | Bytes/sample | Encode MB/s | Decode MB/s |
| --- | --- | ---: | ---: | ---: |
| flat setpoint (f64) | Float64Gorilla | 0.1252 | 14809 | 5062 |
| | **Float64Alp** | 0.0003 | 1460 | 12964 |
| drift 0.1 (f64) | Float64Gorilla | 0.3600 | 10003 | 6035 |
| | **Float64Alp** | 0.0549 | 555 | 9694 |
| drift 0.1 (f32) | Float32Gorilla | 0.2340 | 5359 | 3286 |
| | **Float32Alp** | 0.0549 | 282 | 4863 |
| noisy analog (f64) | Float64Gorilla | 6.8745 | 1680 | 1363 |
| | **Float64Alp** | 6.8299 | 122 | 1015 |
| on/off (bool) | BooleanBits | 0.1251 | 3409 | 1065 |
| | **BooleanRle** | 0.0004 | 3641 | 62771 |
| multi-state (enum) | EnumDict | 0.2501 | 638 | 2789 |
| | **EnumDictRle** | 0.0007 | 689 | 54827 |
| energy counter (i64) | **IntegerDelta** | 0.5158 | 833 | 2743 |
| | IntegerDeltaOfDelta | 0.6397 | 809 | 2503 |
| | IntegerFor | 1.3047 | 1170 | 4043 |
| | IntegerRle | 1.3370 | 647 | 2008 |
| timestamps, regular | **TimestampRegular** | 0.0002 | 29603 | 6625 |
| | TimestampDelta | 0.0470 | 4425 | 6126 |
| | TimestampDeltaOfDelta | 0.0158 | 4145 | 3427 |
| | TimestampFor | 4.7032 | 957 | 1856 |
| | TimestampRle | 4.7188 | 663 | 1206 |
| timestamps, jitter + gaps | **TimestampDelta** | 0.9789 | 924 | 2431 |
| | TimestampDeltaOfDelta | 2.0264 | 873 | 1865 |
| | TimestampFor | 4.7164 | 919 | 1748 |
| | TimestampRle | 4.7320 | 642 | 1198 |

Encode speed of the measured choice, `encode` itself, in MB/s:

| Shape | Encode MB/s |
| --- | ---: |
| flat setpoint | 1309 |
| drift (f64) | 523 |
| drift (f32) | 266 |
| noisy analog | 113 |
| on/off | 1805 |
| multi-state | 667 |
| energy counter | 188 |
| regular timestamps | 14922 |
| jittered timestamps | 212 |

The slowest of these, 113 MB/s (about 14M samples/s), is still about 880×
the 16,000 samples/s target workload (SPEC §15).

zstd seal speed (encoded block → envelope; MB/s of the shape's value bytes,
encode excluded), for the two shapes where zstd does real work:

| Shape | L3 | L4 | L5 | L6 |
| --- | ---: | ---: | ---: | ---: |
| drift 0.1 (f64) | 38098 | 21296 | 21223 | 20455 |
| noisy analog | 1552 | 1546 | 1741 | 1714 |
| jittered timestamps | 2646 | 2790 | 1045 | 972 |

Open + decode stays in the range of the decode column above, from 860 MB/s
(noisy) to 9 GB/s (drift), and does not vary with level.

### Findings and chosen defaults

- **ALP wins on decimal-like data.** On 0.1-resolution drift it is 6.6×
  smaller than Gorilla (0.055 vs 0.36 bytes/sample for `f64`). On a flat
  setpoint it reaches 0.0003 against Gorilla's 0.125. On full-precision noise
  the two are within 1%. `float64::encode` and `float32::encode` therefore
  try both and keep the smaller.
- **Patched FOR** made jittered timestamps 3.2× smaller (3.14 → 0.98
  bytes/sample) compared with plain per-miniblock FOR.
- **Default compression: zstd level 3** (`Compression::Default`).
  - Levels 4–6 gain at most 2% (jittered timestamps 0.522 → 0.511; noisy
    analog 6.70 → 6.69) and nothing elsewhere.
  - Levels 5–6 seal 2.5× slower on timestamps.
  - Level 3 still matters on top of the encodings: drift −30%, energy
    counter −10%, jittered timestamps −47%.
  - `Compression::ZstdSmallest` tries all four levels, for compaction (SPEC
    §42).
- **Encoding before zstd matters.** Measured against zstd 3 on plain
  values, encode + zstd is:
  - 2.3× smaller on drift;
  - 2.2× smaller on the energy counter;
  - 7× smaller on jittered timestamps;
  - over 10,000× smaller on regular timestamps.
- **Codec selection defaults:** `timestamp::encode`, `integer::encode`,
  `boolean::encode`, `enumeration::encode`, `bytes::encode` and the float
  `encode`s, each measuring per block.
