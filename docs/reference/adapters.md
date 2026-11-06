<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Adapters

An adapter turns external data into Norn's entities, signals, samples and metadata,
or Norn's history back into a file another tool reads (SPEC §29, §44, §45). Every
adapter is a client: it reaches the daemon through the public gRPC API and nothing
else (SPEC §47). Implemented: the adapter contract, CSV, Parquet, the Arrow IPC file
format and MCAP, each export and import, driven by
[`nornctl export` and `import`](cli.md#export-and-import). The
[import and export guide](../guides/import-export.md) has real output.

## Where each adapter lives

| Adapter | Crate | State |
| --- | --- | --- |
| The contract | [`adapters/contract`](../../adapters/contract) (`norn-adapter`) | implemented |
| CSV | [`adapters/csv`](../../adapters/csv) (`norn-adapter-csv`) | export and import |
| Parquet, Arrow IPC | [`adapters/parquet`](../../adapters/parquet) (`norn-adapter-parquet`) | export and import |
| MCAP | [`adapters/mcap`](../../adapters/mcap) (`norn-adapter-mcap`) | export and import |
| Device protocols | outside this repository, on [`crates/sink`](../../crates/sink) (`norn-sink`) | clients of the public API over IPC |
| Modbus, OPC-UA, MQTT, custom Protobuf | none | planned |

**Device-protocol adapters live outside this repository** (owner's decision,
2026-11-02, [plan.md](../project/plan.md#external-adapters-owners-request-2026-11-02)).
Such an adapter dials `norn_ipc::connect` and calls `TelemetryService.AppendSamples`,
using only Norn's public client crates. `norn-sink` does that and everything around
it, so an adapter writes only its codec and a mapping
([writing a device adapter](../guides/device-adapter.md)). A device object still maps to an entity, a
signal and annotations such as `protocol=...`, `device_instance=1001`,
`object_type=...`; status flags map onto `Quality`.

`adapters/testkit` is test support: property-test strategies for every value kind.
It is a dev-dependency only.

## The contract

`norn-adapter` owns two traits and two drivers:

| Item | Does |
| --- | --- |
| `SampleSink` | where an export goes: `order()`, `begin(&Catalog)`, `write(&[Sample])`, `finish()` |
| `SampleSource` | where an import comes from: `catalog()`, then `next_batch()` until `None` |
| `export(channel, &Selection, &mut sink, &ExportOptions)` | `ListSignals`, `GetEntity`, `GetStream`, then one `QuerySamples`, streamed into the sink |
| `import(channel, &mut source, &ImportOptions)` | creates what the file declares, then `AppendSamples`, one batch in flight |
| `Catalog` | streams, entities and signals: names, display names, annotations, units, value types, each signal's stream |
| `AdapterError` | `Io`, `Malformed`, `Truncated`, `Corrupt` (each with the `row`), `Encode`, `LimitExceeded`, `Call`, `Refused`, `Conflict`, `Response` |

The vocabulary is `norn-protocol`'s messages, not new types: a file holds what the
wire holds. A catalog never carries times, etags, statistics or a stream's
retention. Those belong to the daemon, and retention is the target operator's choice
(SPEC §43).

A third-party adapter needs only this crate, or only the protocol. Implement a
trait and call a driver, or call `AppendSamples` directly as
[protocol.md](protocol.md#writing-a-client) describes.

### Exactly what an import does

1. `NegotiateProtocol`, requiring `APPEND_SAMPLES`, to learn the published limits.
2. Creates every stream, entity and signal the file declares. `ALREADY_EXISTS` is
   success, and an existing resource's metadata is left alone, since the target's may
   be newer. An existing signal must agree on what can never change: its value type
   and its stream. Otherwise the import fails with `Conflict`, naming the signal.
3. One `QuerySamples` `LATEST` over `entities/-` gives each signal's newest time.
4. For each source batch, samples at or below their signal's newest time are looked
   up with a bounded `QuerySamples` and left out if the daemon holds them. Then the
   rest go in batches of at most `max_batch_samples` and `max_batch_bytes`, one at a
   time, each waiting for its acknowledgement (`DURABLE` by default).
5. `THROTTLE` is waited out. A transient `REJECT` is retried after its `retry_delay`,
   up to `max_retries` (1,000). A permanent `REJECT` stops the import with its reason
   and the first three bad samples.

**Import is idempotent.** The daemon's own duplicate check covers a recent window
only ([storage.md](storage.md#duplicates-exactly-what-is-guaranteed)). A resend older
than the window would be stored again as late data, so an import cannot rely on it.
Step 4 makes a repeat import send nothing it already sent. A fresh import pays one
`LATEST` query and nothing more.

**Damage stops the import at the row.** Everything before that row is stored.
Fix the file, or take the good part, and import again: only what is missing is sent.

### Bounded memory

| Holder | At most |
| --- | --- |
| an export | one `QuerySamples` response (64 MiB decode limit), one Parquet row group (131,072 rows or 64 MiB) or one MCAP chunk (4 MiB), and the catalog |
| an import | one source batch (4,096 rows or 4 MiB of rows), one `AppendSamples` batch in flight, the catalog, and a line per signal |
| a catalog | `DEFAULT_MAX_SIGNALS`, 1,000,000 signals, then `LimitExceeded` |
| a CSV record | 16 MiB (`max_record_bytes`); the first line, the catalog, 1 GiB |
| an MCAP record | 256 MiB, a chunk included, so a damaged length cannot allocate more |

`tests/exchange_memory.rs` exports and imports 100,000 samples, then 400,000. It
checks that the client's peak RSS (from `ps`) does not grow with the data.

## Formats

All four carry every field of a sample: signal, time to the nanosecond, value
exactly, quality (severity and substatus), and sequence. NaN payloads, signed
zeros, infinities, `i64` and `u64` extremes and every quality survive. A property
test checks each format through two real daemons.

### CSV

Long format: one row per sample, RFC 4180, UTF-8, `\n` line ends.

```text
# norn.csv 1 {"version":1,"streams":[{"name":"streams/plant"}],"entities":[{"name":"entities/dev-01"}],"signals":[...]}
time,signal,stream,type,value,severity,substatus,sequence
2026-11-02T08:00:00.000000000Z,entities/dev-01/signals/temperature,streams/plant,double,21.5,GOOD,,0
# norn.end rows=1 crc32=<8 hex digits>
```

| Column | Holds |
| --- | --- |
| `time` | RFC 3339 in UTC. Exports write nine fractional digits; imports take zero to nine. An offset, a leap second or an impossible date is refused, never adjusted |
| `signal` | `entities/{entity}/signals/{signal}` |
| `stream` | `streams/{stream}` |
| `type` | `bool`, `int32`, `int64`, `uint32`, `uint64`, `float`, `double`, `string`, `bytes`, `enum` |
| `value` | see below |
| `severity` | `GOOD`, `UNCERTAIN`, `BAD`; a number this build does not know is written as the number |
| `substatus` | empty for none, else `STALE`, `OVERRIDDEN`, ... or a number |
| `sequence` | a decimal `i64` |

Values: `true`/`false`. Decimal integers, with `uint64` written unsigned. Floats as
the shortest decimal that reads back to the same bits, plus `inf` and `-inf`; `NaN`
for the canonical quiet NaN, and `nan:<hex bits>` for any other NaN. Strings are
quoted when they must be. Bytes as lower-case hex. Enums as their number: the symbol
is display-only and the daemon does not store it.

The `#` lines make an export checkable. The first carries the catalog. The last
holds the row count and a CRC-32 of the row bytes, so a file cut at a row boundary,
or a changed digit, is an error and not a short read. A file with no first `#` line
promises nothing and is accepted. That covers one written by hand, by another tool,
or edited with both lines removed. Its signals are discovered row by row, and a row
contradicting an earlier one (another type or stream) is refused. A file that starts
with the first `#` line and has no last one is truncated.

To read an export with pandas: `pd.read_csv(path, skiprows=1, skipfooter=1,
engine="python", keep_default_na=False)`.

### Parquet

One row per sample, sorted by (signal, time, sequence), with the sort declared in
each row group's `sorting_columns`. Snappy pages (pure Rust); time and sequence
delta-encoded; a row group of at most 131,072 rows.

| Column | Arrow type | Notes |
| --- | --- | --- |
| `signal`, `stream`, `value_type`, `severity`, `substatus` | `Utf8` | words as in CSV |
| `time` | `Timestamp(ns, "UTC")` | |
| `sequence` | `Int64` | |
| `bool_value` | `Boolean`, nullable | |
| `int_value` | `Int64`, nullable | `int32`, `int64`, `uint32`, `enum` |
| `uint_value` | `UInt64`, nullable | `uint64` |
| `float_value`, `double_value` | `Float32`, `Float64`, nullable | bit-exact |
| `string_value`, `bytes_value` | `Utf8`, `Binary`, nullable | |

`value_type` says which value column is set. File key-value metadata holds
`norn.catalog`, the catalog as JSON (the CSV first line's form), and `norn.digests`,
`rows:crc32` per row group. Parquet's writer stores no data checksum, and SPEC §20
will not accept damage silently. The CRC covers each sample's protobuf encoding, so
it checks every field whatever page encoding the file used. A file with these
columns from another tool, and no digests, is read unchecked.

**C:** parquet's default codecs (`zstd`, `lz4`, `brotli`, `flate2`) are off. Only
`snap`, pure Rust, is on. Parquet zstd from the pure-Rust `structured-zstd` is not
possible: parquet 60 names the C `zstd` crate and has no codec hook. So a Parquet
file with zstd pages from another tool fails to import with a clear error.

### Arrow IPC

The same columns and the same metadata keys in the Arrow IPC file format, one record
batch per 16,384 rows, one digest per batch. Uncompressed, since Arrow's IPC codecs
are lz4 and zstd, both C. That makes it the largest format, but nothing decodes it.
`ExportFormat.EXPORT_FORMAT_ARROW_IPC` names it in the schema.

### MCAP

For Foxglove and Lichtblick. Each signal is a channel whose topic is the signal's
name, message encoding `protobuf`, and one shared schema: name `norn.type.Sample`,
encoding `protobuf`, data a `FileDescriptorSet` of `norn/type/sample.proto` and its
imports. Every message is one `norn.type.Sample`, the sample in full. Channel
metadata carries `stream`, `value_type` and `unit`. A metadata record `norn.catalog`,
key `json`, carries the catalog. Messages are in time order, as players read, and
chunks hold 4 MiB with chunk CRCs, message indexes and a summary.

The descriptor is committed at `adapters/mcap/src/generated/norn_type_sample.binpb`;
`just gen` rebuilds it and `just gen-check` compares it. The `mcap` CLI 0.4.0 reads
an export (`mcap info`, `mcap doctor` clean, `mcap cat --json` decodes each message
from the embedded descriptor). Foxglove and Lichtblick decode protobuf channels the
same way; neither app was opened for this.

Limits: `log_time` is unsigned, so a sample before 1970 logs at 0, though its message
keeps the true time. Channel ids are 16 bits, so a file holds at most 65,535 signals.
Chunks are uncompressed: the `mcap` crate's zstd and lz4 are C, so they are off, and
a compressed MCAP from another tool is refused with `Unsupported compression format`.
Each chunk repeats the 18 KB descriptor, so a tiny export is about 39 KB.

## Arrow in memory

`norn_adapter_parquet::columns::schema` and `to_batch` build Arrow `RecordBatch`es,
so a Rust caller can hand samples to an Arrow tool without a file. There is no
Arrow Flight or C data interface, since both need more than these crates.

## ExportStream stays UNIMPLEMENTED

`StreamService.ExportStream` answers `UNIMPLEMENTED`, and that is deliberate. The
server cannot import the adapters: they sit outside it in the layer table
(`xtask/src/arch.rs`), as clients. Writing files on the daemon's host would also need
the daemon to open paths a client names. Export runs in the client: `nornctl export`,
or `norn_adapter::export`, through `QuerySamples`. It has the same bounded memory
and the same ordering, and the file belongs to the user who asked for it. Making
`ExportStream` real means the server importing the adapters, a layer change that has
not been made. `CAPABILITY_EXPORT` is not offered.

## Dependencies (SPEC §39)

| Crate | Why | Features |
| --- | --- | --- |
| `parquet` 60 | the reference Rust Parquet, Apache Arrow project | `arrow`, `snap` only |
| `arrow-array`, `arrow-schema`, `arrow-ipc` 60 | columns and the IPC file format; already in parquet's graph | none |
| `mcap` 0.26 | Foxglove's own MCAP crate, pure Rust with compression off | none |
| `crc32fast` | digests where the format stores none; already in mcap's graph | default |
| `serde_json` | the catalog as JSON; already a workspace dependency | default |

Licences are MIT or Apache-2.0, allowed by `deny.toml`, apart from two recorded
exceptions. `tiny-keccak` (CC0-1.0, a public-domain dedication) is a build-time hash
in a proc macro under arrow's `ahash`. `paste` (unmaintained, RUSTSEC-2024-0436) is a
compile-time macro of `mcap`. Neither is linked into a binary.

arrow's `chrono` reaches `iana-time-zone-haiku`, which builds C++ on Haiku, and
`wasm-bindgen` on wasm. `xtask deps` walks the graph for the targets Norn builds
(`deps::PLATFORMS`: the release matrix and CI's cross targets), so neither appears
and no exception was needed. Tests in `xtask/tests/platform_gates.rs` prove the filter
keeps what ships, Windows-only crates included
([architecture](architecture.md#no-c)).
