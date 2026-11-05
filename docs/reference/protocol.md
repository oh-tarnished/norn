<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Protocol

Norn's public API is the Protobuf schema under [`protobuf/`](../../protobuf). It is the
only way in: the CLI, the MCP server and every adapter speak it, and none
reaches storage another way (SPEC §31, §33, §47). This document says what the
schema contains, what its semantics are in exact words, and why each choice
was made. The `.proto` comments are the per-field reference; where this
document and a comment disagree, fix both.

Implemented: the schema, its lint configuration, the generated Rust in
[`crates/protocol`](../../crates/protocol) (`norn-protocol`), and in the daemon
([`crates/server`](../../crates/server)) `AdminService`, `grpc.health.v1`, the version
metadata check, `EntityService`, `SignalService`, `StreamService` (without
`ExportStream`), `google.longrunning.Operations`, `TelemetryService` (`AppendSamples` and
`SubscribeEvents`), `QueryService` (`QuerySamples`, `BrowseSamples`, `FindEvents`,
`ComputeStats`, `ComparePeriods`), `StateService.ComputeState` and `ReplayService`
(`DriveReplaySession` included). Every
other method answers `UNIMPLEMENTED` (see "Not yet implemented"), so the semantics
below for them are the contract later stages implement.

## Tooling

| Command | What it does |
| --- | --- |
| `buf lint` | buf's `STANDARD` and `COMMENTS` rules: every service, method, message, field, enum, value and oneof is documented. |
| `just api-lint` | Google's [api-linter](https://linter.aip.dev) 2.3.1 over a buf image. |
| `buf breaking --against .git#ref=origin/main` | `WIRE_JSON` breaking-change detection, in CI once `main` carries `protobuf/`. |
| `just gen` | `buf generate` with [`buf.gen.yaml`](../../buf.gen.yaml) into `crates/protocol/src/generated/`, and with [`buf.gen.mcp.yaml`](../../buf.gen.mcp.yaml) (protoc-gen-mcp v1.7.8) into `crates/server/src/mcp/generated/`. |
| `just gen-check` | Regenerates and fails on any diff: a schema edit without `just gen` cannot merge. |

`buf breaking` uses `WIRE_JSON` rather than `FILE` because the 200-line limit
makes moving a message between files routine, and no client can observe it.

The schema imports `buf.build/the-protobuf-project/mcp` (v1.7.3, pinned in `buf.lock`)
for the `(mcp.v1.service)`, `(mcp.v1.tool)` and `(mcp.v1.field)` annotations, which
say which methods an AI agent may call ([mcp.md](mcp.md)). They change no message and
no wire behaviour. One side effect: with that import, protoc-gen-tonic 0.5.0 drops the
doc comment of the generated `StateService::compute_state`; the `.proto` stays its
reference.

Generated code is committed, so building `norn-protocol` needs neither buf,
protoc nor a plugin. The plugins are local and pinned (`protoc-gen-prost`,
`protoc-gen-tonic`, `protoc-gen-prost-crate`, all 0.5.0, emitting prost and
tonic 0.14) because committed output must be reproducible from a checkout. The
tonic stubs are generated with `no_transport`: SPEC §7 puts transports in
`crates/transport`, so a client is built over any `tower` service rather than
tonic's TCP `connect`. `tonic` is used with only its `codegen` feature.

### Disabled lint rules

Every disable is scoped to the fewest files and justified where it is made.

| Rule | Scope | Why |
| --- | --- | --- |
| buf `PACKAGE_VERSION_SUFFIX` | `norn/type` | `norn.type` is an AIP-213 component package, unversioned like `google.type`; AIP-215 exempts only a `.type` package from its ban on foreign type references. |
| buf `RPC_REQUEST_RESPONSE_UNIQUE`, `RPC_RESPONSE_STANDARD_NAME` | all | AIP-131/133/134 return the resource from Get/Create/Update, AIP-135 `Empty` from Delete, AIP-151 `Operation` from long-running methods. buf's wrapper-per-method rule contradicts the AIPs, which are the authority here. |

The AIP linter runs with no configuration and nothing disabled. Comment-level
disables are not used: an escape hatch inside the file under review is not a
review.

### Unsigned values

AIP-141 forbids unsigned wire types, and SPEC §10 requires unsigned values, so:

| Field | Type | Meaning |
| --- | --- | --- |
| `Value.uint32_value` | `int64` | 0 to 2^32 - 1, which `int64` holds exactly; anything else is rejected |
| `Value.uint64_value` | `int64` | the same 64 bits as the `u64`. The wire encoding is identical to `uint64`'s; a value of 2^63 or more reads as negative in the JSON mapping. The signal's `ValueType` says which reading applies |
| `Sample.sequence` | `int64` | zero or more; negative is rejected. norn-core's `Sequence` is `u64`, so converting one above `i64::MAX` to the wire is refused |
| `EnumValue.number` | `int32` | zero or more; negative is rejected. norn-core's enum ordinal is `u32`, so converting one above `i32::MAX` to the wire is refused |

## Packages and resources

| Package | Contents |
| --- | --- |
| `norn.type` | `Value`, `EnumValue`, `ValueType`, `Quality`, `Sample`, `ErrorReason` |
| `norn.entity.v1` | `Entity` (`entities/{entity}`), `EntityService` |
| `norn.signal.v1` | `Signal` (`entities/{entity}/signals/{signal}`), `SignalService` |
| `norn.stream.v1` | `Stream` (`streams/{stream}`), `StreamService`, compaction and export operations |
| `norn.telemetry.v1` | `TelemetryService`: `AppendSamples`, `SubscribeEvents`, `Event`, backpressure and acknowledgement |
| `norn.query.v1` | `QueryService`: `QuerySamples`, and the bounded unary reads `BrowseSamples`, `FindEvents`, `ComputeStats`, `ComparePeriods` |
| `norn.replay.v1` | `ReplaySession` (`replaySessions/{replay_session}`), `ReplayService` |
| `norn.state.v1` | `StateService.ComputeState` |
| `norn.admin.v1` | `Server` (singleton `server`), `AdminService`: `GetServer`, `NegotiateProtocol` |

Resource types use the service name `norn.local` (`norn.local/Entity`). Norn is
reached over local IPC, so the name identifies the API, not a host; `.local`
claims no public domain.

A signal is a child of its entity because it means nothing without it: the
same property of two machines is two signals. A stream is top-level: it is the
unit of storage, retention, compaction and export, and a signal names the
stream it records into (immutable, since moving it would split its history).

**SPEC §9's NornService** is realised as three per-package services,
`EntityService`, `SignalService` and `StreamService`. A single service would
have to live in one package while owning resources of three, and would become
the one giant service §9 rules out.

Every collection has the AIP standard methods: Get, List (`page_size`,
`page_token`, AIP-160 `filter`), Create (with a client-chosen ID and
`validate_only`), Update (`update_mask`, AIP-154 `etag`) and Delete. Where a
collection spans entities, the AIP-159 wildcard parent `entities/-` reads
across all of them.

**Batch create (AIP-233).** Entities and signals also have
`EntityService.BatchCreateEntities` and `SignalService.BatchCreateSignals`, for a
client that provisions a whole deployment at start: a device adapter creates about
60,000 signals for 1,000 devices, and one create per call dropped readings while it ran. A
batch holds 1 to 1,000 `Create...Request`s and fits `Limits.max_message_bytes`, 256 KiB.
`BatchCreateSignals` takes a parent of `entities/{entity}`, or `entities/-` when the
signals belong to several entities. A child request's `parent` is then empty or equal
to the batch's, or under `entities/-` names its own entity. A child's `validate_only`
must be unset; the batch has its own.

- **All or nothing, one commit.** The daemon checks every request, then makes the
  whole batch one registry log write and one fsync. Any failure creates nothing; a
  crash keeps the whole batch or none of it.
- **Safe to repeat.** A resource that already exists is returned as stored, its
  display name, unit and annotations left alone, rather than failing the batch. This
  is the per-call rule ("`ALREADY_EXISTS` means a previous run did") applied to a
  batch, where one existing resource must not sink the other 999. A signal must still
  agree on its value type and stream; one that does not fails the batch with
  `ALREADY_EXISTS`, naming it.
- **Errors name the request.** A malformed request fails with `INVALID_ARGUMENT`
  whose `field` is its path, such as `requests[17].signal_id`. A resource given twice
  in one batch, an empty batch and one over 1,000 fail on `requests`. A missing entity
  or stream is `NOT_FOUND`, and passing `max_entities` or `max_signals` is
  `RESOURCE_EXHAUSTED`.
- **Not MCP tools.** Both write, so their `(mcp.v1.tool)` hints say
  `read_only: false`, and the daemon's MCP surface serves only read-only tools.

The daemon implements this subset of AIP-160: comparisons `field = "value"`
joined by `AND`, over `display_name`, `annotations.<key>` and, on signals,
`id`, `unit`, `stream` and `value_type`. Anything else fails with `INVALID_ARGUMENT`
rather than being ignored. A page token is opaque, names the last resource
returned, and is bound to the filter and parent: changing either fails with
`ERROR_REASON_INVALID_TOKEN`. Pages reflect concurrent changes; they are not a
snapshot. `update_mask` names fields by their proto name, and an empty mask means
every field the request populated, except that an empty field then leaves the
value alone (AIP-134). Naming an immutable field fails with `INVALID_ARGUMENT`.

**Delete never deletes history.** Deleting an entity with signals, a signal
with stored samples, or a stream with samples or signals fails with
`FAILED_PRECONDITION` and `ERROR_REASON_RESOURCE_NOT_EMPTY`. Removing history
is the stream's explicit `RetentionPolicy` (SPEC §43), applied at whole-segment
granularity; an unset policy keeps everything.

Free-form metadata is `annotations` (`map<string, string>`): source
protocol, addressing and units belong there, set by adapters. Norn indexes
the keys for filtering and never interprets them; no domain word appears in
the schema (SPEC §4).

## Values and samples

`Value` is a oneof of `bool`, `int32`, `int64`, `uint32`, `uint64`, `float`,
`double`, `string`, `bytes` and `EnumValue`. A signal fixes its `ValueType` at
creation, because stored chunks are encoded for it. Floats are stored
bit-exactly, NaN payloads included. A new type is a new oneof member, so an
older peer sees an unknown field and rejects the value rather than misreading
it. `EnumValue` carries the member `number`, which Norn stores and compares,
and an optional `symbol` for display only.

`Sample` is `signal`, `observe_time` (the source's time, nanosecond
precision, never rewritten), `value`, `quality` and `sequence`. Samples of one
signal order by `observe_time`, then `sequence`. A sample equal to a stored
one in signal, time and sequence is a duplicate: it is counted and not stored
again, which makes retrying a batch safe. [storage.md](storage.md#duplicates-exactly-what-is-guaranteed)
says exactly how far that holds in bounded memory: a resend older than the
recent-sample window, below the signal's high-water mark, is stored as late data.

`Quality` mirrors norn-core: a `QualitySeverity` (`GOOD`, `UNCERTAIN`, `BAD`)
and a `QualitySubstatus` saying why. Quality never alters a value; a `BAD`
sample is still history.

## Ingestion: `TelemetryService.AppendSamples`

A bidirectional stream. Each `AppendSamplesRequest` is one batch: a `stream`,
a client-chosen `batch_id` (unique and increasing within the call), the
samples, and the `ack_level` the client wants to hear about. Bidirectional so
a client keeps several batches in flight and learns of backpressure without
waiting on each write; bounded by `Limits.max_pending_batches`.

**A batch is atomic.** It is admitted whole or rejected whole. A batch names
exactly one stream because backpressure is decided per stream (SPEC §17).

**Responses.** Every batch gets a first response as soon as it is admitted or
rejected, carrying its `Admission` and the level reached so far. If it was
admitted below the requested level, exactly one more response follows: at the
requested level, or with `error` set if it can no longer reach it. First
responses arrive in batch order, and so do second responses within a stream.
Closing the request side lets in-flight batches finish; the daemon then closes
its side. From 1.0.0-rc.1, a daemon that starts shutting down stops reading new batches
on an open call, answers every batch it already admitted, and then ends the call with
`UNAVAILABLE` and `ERROR_REASON_SHUTTING_DOWN`; a client reconnects and resends
what was not acknowledged, as after any broken call. A call held open between batches
therefore no longer delays a stop. As implemented, second responses arrive in batch order across the whole
call, a stricter order than promised. An admitted batch's first response always
reports `BUFFERED`, the level admission itself reaches, so a batch asking for
`RECEIVED` or `BUFFERED` gets one response.

A batch that names no existing stream, or sets no acknowledgement level, or has
a `batch_id` not above the call's last, or would pass `max_pending_batches`, is
rejected, with `error` saying why: `ERROR_REASON_RESOURCE_NOT_FOUND`,
`ERROR_REASON_INVALID_ARGUMENT` or `ERROR_REASON_LIMIT_EXCEEDED`. A sample whose
signal records into another stream is an invalid sample.

### Backpressure outcomes (SPEC §18)

Every admission carries an outcome and a reason, never `UNSPECIFIED`.
Telemetry is never silently dropped or delayed.

| Outcome | Exact meaning |
| --- | --- |
| `ACCEPT` | Admitted, with capacity to spare. Keep sending at the current rate. Reason `CAPACITY_AVAILABLE`. |
| `BUFFER` | Admitted into a write buffer the write path has not caught up with: acknowledgement beyond `BUFFERED` will lag. Expect latency, not loss. Reason `WRITE_PATH_BEHIND`. |
| `THROTTLE` | Admitted, but the stream's buffer passed its high-water mark (or this client its rate). The client must not send another batch for this stream until `retry_delay` has elapsed; one sent sooner is rejected with reason `THROTTLED`. |
| `REJECT` | Refused: no sample of the batch is stored. Transient reasons (`BUFFER_FULL`, `MEMORY_EXHAUSTED`, `THROTTLED`, `SHUTTING_DOWN`, `CLIENT_RATE_EXCEEDED`) carry a `retry_delay` after which the same batch may be retried. Permanent ones (`STORAGE_UNAVAILABLE`, `BATCH_TOO_LARGE`, `INVALID_SAMPLES`, `ACK_LEVEL_UNAVAILABLE`) need the batch or the configuration changed. `INVALID_SAMPLES` lists every bad sample in `sample_errors`. |

`CLIENT_RATE_EXCEEDED` is in the schema but no configuration sets a client rate
yet, so nothing sends it. `THROTTLE`'s block is per stream, not per client: any
batch for that stream during the delay gets `THROTTLED`.

`ACCEPT`, `BUFFER` and `THROTTLE` all admit the batch: every sample in it will
be stored. Throttling the *next* batch rather than refusing *this* one means a
well-behaved client never resends data it already transferred.

### Acknowledgement levels (SPEC §19)

Each level implies every level below it.

| Level | Exact meaning | Survives |
| --- | --- | --- |
| `RECEIVED` | The daemon has read and validated the whole batch; it is in process memory. | nothing |
| `BUFFERED` | The batch is in its stream's bounded write buffer, and its order relative to every other batch of the stream is fixed. | nothing beyond the process |
| `PERSISTED` | The batch is written to Norn's storage files (the stream's journal) and handed to the operating system. | the daemon process exiting or crashing |
| `DURABLE` | The batch has crossed the configured durability boundary: its data and the metadata needed to recover it are flushed to stable storage (fsync or the platform equivalent). | power loss, to the extent the device honours flushes |

`DURABLE` is reported only once that boundary is met. A daemon configured
without one does not offer `CAPABILITY_ACK_DURABLE`, and a batch requesting it
is rejected with `ACK_LEVEL_UNAVAILABLE` rather than acknowledged at a weaker
level.

## Subscriptions: one event model (SPEC §27)

`TelemetryService.SubscribeEvents` serves both modes; each response carries a
batch of `Event`s. An `Event` has the `mode`, an `event_sequence` (1, then
increasing by exactly one), the `clock_time` (the daemon's wall clock for
`LIVE`, the session's virtual clock for `REPLAY`), and one of: a `Sample`,
exactly as ingested; a `ClockEvent` (`RUNNING`, `PAUSED`, `SEEKED`,
`SPEED_CHANGED`, `ENDED`, `STOPPED`); or, since 1.3, a `state_sample`. A consumer
written against live data runs unchanged against history.

- `LIVE` follows every sample admitted after the subscription starts,
  selected by `parent` (`entities/{entity}` or `entities/-`) and `filter`.
  Its queue is bounded; a subscriber that falls behind is ended with
  `RESOURCE_EXHAUSTED` and `ERROR_REASON_SUBSCRIBER_TOO_SLOW`, never served
  a gap.
- `REPLAY` follows a replay session. The session's clock slows to its
  slowest subscriber, because replay is deterministic and its consumers must
  see every event. A sample event's `clock_time` is its `observe_time`. After a
  `SEEKED` event comes one `state_sample` per selected signal that had a sample
  before the target: its newest, from `StateAt`, so folding the stream gives the
  state at the clock across seeks. `STOPPED` is the last event; the stream then
  closes. `ENDED` does not close it, since a seek brings more.

As implemented: a live subscription resolves its signals when it starts, and
delivers samples once they are journalled, before the journal flush that makes them
`PERSISTED`. Both modes together are bounded by `Limits.max_subscriptions`; past it,
`RESOURCE_EXHAUSTED` and `ERROR_REASON_CAPACITY_EXHAUSTED`. Fields of the other mode,
or no mode, are `INVALID_ARGUMENT`. [replay.md](replay.md#subscriptions-spec-27) has
every rule and bound.

## Replay (SPEC §25, §28)

A `ReplaySession` is a resource: an immutable `interval` (half-open, both ends
required), `scope` and `filter`; a mutable pace (`speed`, a multiple of real
time where 0 freezes the clock, or `max_speed`, emitting as fast as
subscribers read); and output state `state`, `current_time` (the virtual
clock), `emitted_count` and `expire_time`.

Sessions start `PAUSED` at the interval's start, so subscribers attach before
anything is emitted. The clock controls are AIP-136 custom methods, each a
state transition returning the session:

| Method | Transition |
| --- | --- |
| `:pause` | `RUNNING` → `PAUSED`; no-op when paused |
| `:resume` | `PAUSED` → `RUNNING`; no-op when running |
| `:seek` | moves the clock within the interval, emitting nothing skipped; `ENDED` → `PAUSED` |
| `:step` | from `PAUSED`, emits the next `event_count` events or `duration` of virtual time, stays `PAUSED` |
| `:stop` | → `STOPPED`, terminal; subscribers get `STOPPED` and close |

A control the state forbids fails with `FAILED_PRECONDITION` and
`ERROR_REASON_REPLAY_STATE_CONFLICT`. Speed changes go through
`UpdateReplaySession`. Every control accepts an `etag`, so two controllers
cannot interleave unknowingly.

**Determinism.** A session emits the selected samples with `observe_time` in
its interval, ordered by `observe_time`, then `sequence`, then signal name.
The events and their order depend only on the interval, scope, filter, seeks
and steps, never on speed, pauses, or subscriber timing.

Sessions are bounded (`Limits.max_replay_sessions`) and expire after
`Limits.replay_session_idle_timeout` without a call or a live subscription.

As implemented: an etag is `"g<generation>"`. A step answers once its events have
reached every subscriber; while one is delivering, every control but stop is a state
conflict. Sessions live in memory and end at restart. The design and every bound:
[replay.md](replay.md).

## State at an instant (SPEC §23, §26)

`StateService.ComputeState` is SPEC's `StateAt(T)`: for each selected signal,
the latest sample with `observe_time` at or before `state_time`, ties broken
by the highest `sequence`; unset if there is none, or if it is older than
`max_age`. It equals what replaying every selected sample up to and including
`state_time` leaves behind (SPEC §36). It is named `ComputeState` because
AIP-136 keeps prepositions out of method names. It is paginated, and the page
token pins the snapshot, so pages agree even if late samples arrive between
calls. Checkpoints and chunks never appear in the API.

As implemented, the first page computes the whole answer and the daemon holds it,
charged to the query budget, until its last page is read or it has been idle 60 s; at
most 16 are held, the oldest evicted first. A token whose answer has gone, or that was
issued for different fields (all but `page_size` and `page_token`), fails with
`INVALID_ARGUMENT` and `ERROR_REASON_INVALID_TOKEN`; start from the first page again.
[query.md](query.md#state-at-an-instant) has the exact rules.

## Query (SPEC §22)

`QueryService.QuerySamples` is server-streaming, so a result of any size flows
in bounded memory on both sides: the daemon never builds the whole result, and
a consumer that stops reading stalls the query rather than growing a buffer.
It selects signals by `parent` and `filter`, samples by a half-open `interval`
and an AIP-160 `sample_filter` over value and quality, and either returns raw
samples (`SAMPLE_ORDER_BY_SIGNAL`, the storage order, or `_BY_TIME`, the
replay order) or an `Aggregation` (`LATEST`, `FIRST`, `LAST`, `COUNT`, `MIN`,
`MAX`, `AVERAGE`, `SUM`, `RATE`) over windows of a fixed width aligned to the
interval's start, or to the Unix epoch when it has none. Empty windows produce
nothing. Each response carries a `resume_token`, so a broken stream continues where
it stopped.

Every sample acknowledged at `PERSISTED` or better is visible, buffered or stored,
exactly once. A query returns each (signal, time, sequence) once. `RATE` is a
counter's rate: a decrease is a reset. A chunk failing its checksum ends the query
with `DATA_LOSS` and is never skipped. [query.md](query.md) has every exact rule:
the filter language, NaN, infinities and signed zero, and resume tokens.

### Bounded unary reads

`QuerySamples` is the bulk path: lossless, streamed, any size. Beside it, every read an
AI agent or a simple client needs is also a unary call with a bounded answer, so it is
usable from any gRPC client and becomes an MCP tool unchanged ([mcp.md](mcp.md)):

| Method | Answers | Page |
| --- | --- | --- |
| `BrowseSamples` | one page of what `QuerySamples` streams, raw or aggregated, such as `LATEST` | 100 results, at most 1,000 |
| `FindEvents` | samples passing `sample_filter` (`EVENT_KIND_MATCH`), or whose value (`_VALUE_CHANGE`) or quality (`_QUALITY_CHANGE`) differs from the signal's previous sample | 100 events, at most 1,000; 1,000,000 samples scanned per call |
| `ComputeStats` | per signal: count, min, max, average, sum, first and last | 20 signals, at most 50 |
| `ComparePeriods` | those for a baseline and a comparison interval, and the change | 20 signals, at most 50 |

Every answer also stops at about 256 KiB once it holds one result, so a call always moves
forward. A page token names the last result returned (or for `FindEvents`, the last
sample read and the value or quality it compares the next one with), never a count, and
is bound by a fingerprint to the request's other fields: `page_size` and
`max_value_bytes` may change between pages, nothing else. A string or bytes value longer
than `max_value_bytes` (1,024 by default, at most 65,536) is cut and flagged
(`truncated_indexes`, `value_truncated`); `QuerySamples` never cuts.

`FindEvents` compares values bit for bit, by their stored bytes: -0.0 differs from 0.0,
a NaN equals the same NaN, an enum compares by number. "Previous" is the previous
sample of the same signal within the interval passing `sample_filter`; a signal's first
such sample is the baseline, never a change. Events come grouped by signal, in time
order, so one previous sample is all a search holds. A call that reaches the scan bound
returns what it found, possibly nothing, with a token.

`ComputeStats` is the engine's own `COUNT`, `MIN`, `MAX`, `AVERAGE`, `SUM` and `LATEST`
over a page of signals, plus one raw read per signal for its first sample's time:
several reads, not one snapshot, so on a signal still receiving samples an interval
reaching the present can see a sample in one figure and not another.

`ReplayService.DriveReplaySession` moves a frozen (speed 0) session by `event_count`,
`duration` or `seek_time` and returns what that emitted, as a subscriber would have
received it: it attaches, moves the clock, and reads up to the `SPEED_CHANGED` that
setting speed 0 again sends after everything the movement emitted. A step that would
emit more than `max_events` (100 by default, at most 1,000) is refused with
`ERROR_REASON_LIMIT_EXCEEDED` before the clock moves; a seek's state events past it
are counted in `omitted_event_count`. A session that is not frozen is a state conflict.

## Long-running operations

`StreamService.CompactStream` (`:compact`), `VerifyStream` (`:verify`),
`ApplyStreamRetention` (`:applyRetention`) and `ExportStream` (`:export`, to CSV,
Parquet, Arrow IPC or MCAP at a `file://` URI that must not exist) return
`google.longrunning.Operation` with typed response and metadata. The daemon serves
`google.longrunning.Operations` to poll them: `GetOperation`, `ListOperations`
(oldest first, no filter), `WaitOperation` (at most 60 s, then the operation as it
stands), `CancelOperation` and `DeleteOperation` (`FAILED_PRECONDITION` while it runs).

**ExportStream stays `UNIMPLEMENTED`, by decision.** The formats live in client
crates (`adapters/`) that the server may not import, by the layer table. Export is
done client-side instead: `nornctl export`, or `norn_adapter::export`, reads through
`QuerySamples` in bounded memory and writes the file as the user who asked. Import is
client-side too, through `AppendSamples`. Serving `ExportStream` would need a layer
change; see [adapters.md](adapters.md#exportstream-stays-unimplemented).

Compaction is lossless and crash-safe, and ingestion continues during it (SPEC §42).
`interval` restricts it to segments overlapping that interval. Segments holding known
damage are left alone and counted in `skipped_segment_count`.

`VerifyStream` reads every chunk of every sealed segment and changes nothing. From
1.0.0-rc.1 it also reads the chunks already fsynced to the open segment, checking each
frame's header and CRC; samples still buffered in memory are in no file and are not
examined. `segment_count` counts sealed segments only, while `chunk_count` and
`sample_count` include the open segment's chunks. Finding
damage is a *successful* operation: the result is the answer asked for.

`ApplyStreamRetention` runs the recorded policy now. `validate_only` removes nothing
and reports what would go; a dry run and the pass after it list the same segments if
nothing changed between them. The daemon also applies policies on a schedule.

As implemented: one worker runs one job at a time, and at most 16 wait; past that a
call fails with `RESOURCE_EXHAUSTED` and `ERROR_REASON_CAPACITY_EXHAUSTED`, and during
shutdown with `UNAVAILABLE` and `ERROR_REASON_SHUTTING_DOWN`. Operations live in
memory, at most 256, so a restart gives `NOT_FOUND`. Cancelling stops a compaction
before its commit and a verification between segments; a retention pass that has begun
deleting finishes. A cancelled operation ends with `CANCELLED`, and a failed one with
`INTERNAL` and `ERROR_REASON_STORAGE_UNAVAILABLE`.

[storage.md](storage.md#maintenance) has the design.

## Error model

Errors follow AIP-193: a `google.rpc.Status` whose details hold exactly one
`google.rpc.ErrorInfo` with `domain` `norn.local` and `reason` the name of a
`norn.type.ErrorReason` value, such as
`ERROR_REASON_RESOURCE_NOT_EMPTY`. The status code gives the class of failure;
the reason says which one; metadata carries the specifics (`limit`, `chunk`,
`capability`). Callers branch on (domain, reason), never on message text. A
caller seeing an unknown reason acts on the code alone.

Corrupted data is reported (`DATA_LOSS`, `ERROR_REASON_DATA_CORRUPTED`) and
never silently skipped (SPEC §20).

In Rust, `norn_protocol::rpc` is `tonic-types`: its `pb::Status` and
`pb::ErrorInfo` are the schema's `google.rpc` types (buf.gen.yaml maps them
there), and its `ErrorDetails` and `StatusExt` put them into a
`tonic::Status`.

### Not yet implemented

Every service in the schema is registered, so a method the daemon lacks fails
with `UNIMPLEMENTED` and `ERROR_REASON_NOT_IMPLEMENTED`, with metadata `service`,
`method`, and `capability` where one gates the service. A method outside the
schema gets tonic's bare `UNIMPLEMENTED`: the daemon has no name for it.

### Malformed arguments

A malformed field fails with `INVALID_ARGUMENT`, `ERROR_REASON_INVALID_ARGUMENT`
with metadata `field`, and also a `google.rpc.BadRequest` field violation.

### Message limits

A request is decoded only after its size is reserved from the IPC memory budget,
as it arrives. A message above the method's limit (`Limits.max_batch_bytes` for
`AppendSamples`, `Limits.max_message_bytes`, 256 KiB, for every other method, streaming
or not) fails with
`RESOURCE_EXHAUSTED` and `ERROR_REASON_LIMIT_EXCEEDED` before it is buffered. When
the budget stays full past a bound, the call fails with `RESOURCE_EXHAUSTED` and
`ERROR_REASON_CAPACITY_EXHAUSTED`. A batch with too many samples is counted
before it is decoded and rejected with `BATCH_TOO_LARGE`; the call goes on. The size
refusal's metadata names the `limit` it met.

Size batches from these fields, never from a constant. A `BatchCreateEntities` or
`BatchCreateSignals` request must fit both `max_batch_create_requests` entries and
`max_message_bytes` encoded bytes, and at scale the bytes run out first: split on the
request's encoded length. An `AppendSamples` batch must fit `max_batch_samples` and
`max_batch_bytes`. A daemon before 1.8 leaves `max_message_bytes` and
`max_batch_create_requests` zero and enforces 262144 and 1000.

## Inspecting storage

`StreamService.ListSegments` and `GetSegment` (1.8) say how a stream's history sits on
disk, and change nothing. `SEGMENT_VIEW_BASIC`, the default, answers from the temporal
index and reads no chunk. `SEGMENT_VIEW_FULL` also reads and checks every chunk of the
segment for its signals, codecs, compression and decoded size, which costs a read of the
whole file, so a FULL list page holds at most 10 segments. Segments come and go with
sealing, compaction and retention: a name read now may answer `NOT_FOUND` at the next
call, and a FULL list skips a segment that went while it was being read. Neither method
is an MCP tool. A damaged chunk is counted in `damaged_chunk_count`, not an error;
`VerifyStream` says where it is.

## Versioning and capability negotiation (SPEC §40)

A protocol version is `major.minor`. `major` is the `vN` of every `norn.*`
package, all versioned in lockstep; it changes only with a new package
version, which is the only place wire-incompatible changes may happen. `minor`
counts compatible additions within a major: methods, fields, enum values,
error reasons, capabilities. `buf breaking` enforces that a minor never breaks
the wire or JSON form. The published v0.1.0 release speaks 1.6; 1.0.0-rc.1 speaks 1.7, and this tree
1.8 (`norn_protocol::PROTOCOL_VERSION`).
The protocol crate is versioned `1.0.0-rc.1` with the rest of the workspace;
this release identifier is separate from the integer major/minor wire version.
1.1 added error reasons, `Server.recovery` and
`Server.index` to 1.0. 1.2 added `StreamStats.buffered_sample_count` and
`corrupt_sample_count`, changed `sample_count` to include buffered samples and
exclude corrupt ones, and defined `AGGREGATE_FUNCTION_RATE` over counter resets;
nothing had implemented that function, so no client can have relied on the old text.
1.3 added `Event.state_sample`, sent after a seek. 1.4 added `VerifyStream`,
`ApplyStreamRetention`, `RetentionPolicy.max_months`,
`CompactStreamResponse.skipped_segment_count`, `Server.maintenance`, three
`RecoveryReport` counts, and `CAPABILITY_RETENTION` and `CAPABILITY_VERIFY`. Under 1.4
`corrupt_sample_count` counts damage as soon as anything finds it, not at the next
start, and retention's unit is the segment, as it always had to be. 1.5 added
`BrowseSamples`, `FindEvents`, `ComputeStats`, `ComparePeriods` and
`DriveReplaySession`, and the `(mcp.v1.*)` annotations, which change no message.
1.6 added `BatchCreateEntities` and `BatchCreateSignals`. A client learns it may use
them from an agreed minor of 6 or more; a 1.5 daemon answers them `UNIMPLEMENTED`.

1.7 (Norn 1.0.0-rc.1) adds the opt-in [shared-memory](shared-memory.md) data plane: the
message `norn.admin.v1.SharedMemoryPlane` (`service`, `root_directory`, `name_prefix`,
`max_frame_bytes`, `slot_count`, `max_rings`, `segment_bytes`) and the field
`NegotiateProtocolResponse.shared_memory`. The daemon sets that field only when it
offers `CAPABILITY_SHARED_MEMORY` and the agreed minor is 7 or more, so an older
client never sees it. A ring is asked for and granted per `AppendSamples` call in gRPC
metadata (`norn_protocol::DATA_PLANE_METADATA_KEY` and
`SHARED_MEMORY_SERVICE_METADATA_KEY`): the request key `norn-data-plane` with the
value `shared-memory`, and the response key `norn-shared-memory-service` naming the
ring's whole service, `{service}/{ring}`. Without that answer the call stays on gRPC.
Every response still travels on the gRPC call, so acknowledgements, backpressure and
errors are unchanged.

1.8 adds `Limits.max_message_bytes` and `Limits.max_batch_create_requests` (see
"Message limits"), and `StreamService.GetSegment` and `ListSegments` (see "Inspecting
storage"). The limits themselves are not new: a 1.7 daemon enforces the same values
without publishing them.

A client calls `AdminService.NegotiateProtocol` once per connection, before
anything else, with its version and the capabilities it cannot work without:

- If the majors differ, the call fails with `FAILED_PRECONDITION` and
  `ERROR_REASON_PROTOCOL_VERSION_UNSUPPORTED`, ErrorInfo metadata giving the
  server's version.
- If a required capability is missing, it fails with `FAILED_PRECONDITION`
  and `ERROR_REASON_CAPABILITY_UNAVAILABLE`, naming the capability.
- Otherwise it returns the agreed version (the shared major and the lower
  minor), every capability offered, the `Limits`, and the daemon's release.
  The client must not use anything newer than the agreed version.

A client may also send the gRPC metadata
`norn-protocol-version: <major>.<minor>` on any call; a daemon that cannot
speak that major, or cannot parse the value, fails the call the same way.
`GetServer` is exempt from this check and needs no negotiation, so any client,
however old, can learn what it is talking to. The
aim is that an old client fails clearly at connection time instead of
misbehaving halfway through a task.

Capabilities describe what this daemon's build and configuration offer:
`APPEND_SAMPLES`, `ACK_PERSISTED`, `ACK_DURABLE`, `LIVE_SUBSCRIPTION`,
`REPLAY`, `STATE_AT`, `QUERY_RANGE`, `QUERY_AGGREGATES`, `COMPACTION`,
`EXPORT`, `SHARED_MEMORY`, `RETENTION`, `VERIFY`. Clients ignore numbers they do not know.

`Server.recovery` says what startup recovery found and how much it read,
`Server.index` what opening the temporal index found ([storage.md](storage.md)), and
`Server.maintenance` what compaction and retention have done since the start.

`Limits` are published, not discovered by failure: batch size in samples and
bytes, pending batches per call, streams, replay sessions, subscriptions, the
replay idle timeout, and from 1.8 the largest request of any other method and the
most requests in a batch create (SPEC §16, §41). See "Message limits".

The daemon offers `APPEND_SAMPLES`, `ACK_PERSISTED`, `QUERY_RANGE`,
`QUERY_AGGREGATES`, `STATE_AT`, `LIVE_SUBSCRIPTION`, `REPLAY`, `COMPACTION`,
`RETENTION`, `VERIFY`, and `ACK_DURABLE` unless `[storage] durable_acks = false`.
From 1.0.0-rc.1 it also offers `SHARED_MEMORY` when `[shared_memory] enabled = true` and the
endpoint is a Unix socket. `Limits` are all set and all
enforced. `max_batch_bytes` is
the 4 MiB gRPC message limit, lowered to an eighth of `ipc_bytes` if that is
smaller, so one maximal batch fits the decoding half of the budget twice over.

Health is the standard `grpc.health.v1` service, not a Norn method. The daemon
reports the service names `""` and `norn.admin.v1.AdminService`, each `SERVING`
only while the runtime is serving.

## Writing a client

An adapter needs `norn-ipc` and `norn-protocol`, with `tonic` and `tokio`, and
nothing of the daemon. [`crates/cli/examples/plain_client.rs`](../../crates/cli/examples/plain_client.rs)
is a whole one, run against a real daemon by nornctl's test.

1. `norn_ipc::connect(&endpoint)`, with `Endpoint::platform_default()` or a parsed
   `unix:`, `pipe:` or `tcp:` string. The channel is cheap to clone and reconnects
   by itself. Optionally `NegotiateProtocol`, requiring
   `CAPABILITY_APPEND_SAMPLES` and the acknowledgement capability it relies on.
2. Create the stream, entities and signals. `ALREADY_EXISTS` means a previous run
   did: create is safe to repeat at every start. A signal's value type is fixed.
   With more than a few hundred, use `BatchCreateEntities` and `BatchCreateSignals`
   (from 1.6) with up to 1,000 a call: a batch returns existing resources rather than
   failing, so the whole catalogue can be sent at every start.
3. Open one `AppendSamples` call and keep it open, feeding it from a bounded
   channel. `batch_id` restarts per call and must increase. Read responses
   concurrently with sending. On `THROTTLE`, wait `retry_delay` before the next
   batch; on a transient `REJECT`, wait it and resend that batch.
4. If the call or the daemon dies, reconnect, open a new call, and resend every
   batch not yet acknowledged at the level you asked for. Giving each sample a
   stable `observe_time` and `sequence` makes that safe (duplicates, above).

A connection silent for 10 minutes is closed by the daemon. A client with less to
say should set HTTP/2 keep-alive on its channel with `norn_ipc::connect_with`.

## HTTP mapping

Every unary and server-streaming method carries a `google.api.http` binding,
as AIP-127 requires, so the schema can be transcoded to REST later. Norn
serves gRPC only today (and MCP, generated from the same schema). `AppendSamples` has none: AIP-127 leaves
bidirectional streams to gRPC.
