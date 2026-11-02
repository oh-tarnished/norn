# Norn — Master Architecture & Implementation Specification

The authoritative brief for Norn, as given by the project owner on 2026-11-01.
Every agent and contributor reads this before changing anything. Where the
code and this document disagree, update the design docs first, then the code.

Norn is a high-performance, cross-platform temporal data engine: it ingests,
stores, compresses, indexes, queries, replays, reconstructs and streams
historical and live data. The first workload is building-management telemetry
(BACnet), but **Norn itself is domain-independent**. Future sources: BACnet,
Modbus, OPC-UA, MQTT, robotics, industrial systems, vehicles, IoT, simulation,
custom sensors, arbitrary Protobuf/event streams.

> **Norn stores the history of systems and provides a deterministic interface to
> understand what happened, what is happening, and what the system looked like
> at any point in time.**

## 1. Language
Pure Rust. No C, C++, Go, Python, Java or Node.js in the core. External SDKs and
adapters may exist later; the engine is Rust. Build with Rust 1.99.1 (pinned in `rust-toolchain.toml`) and
well-maintained crates. Avoid `unsafe` unless a performance or OS-integration
need is demonstrated; every `unsafe` is isolated, documented, justified,
reviewed and tested.

## 2. File size
Every source file stays **under 200 lines of code** — a hard constraint. Near
the limit: split, add a submodule, extract a trait or protocol type, extract
tests, separate platform code. Never compress code artificially. Tests may be
split into separate files. Documentation files are exempt.

## 3. Documentation
Every public module, type and function has Rustdoc covering purpose,
invariants, ownership, concurrency, failure behaviour, performance and safety
assumptions, with examples where useful. Maintain one `README.md` at the root
(no other README anywhere), `CONTRIBUTING.md` and `SECURITY.md` beside it, and
everything else under `docs/`: `docs/guides/` (getting-started, configuration,
ingesting-data, querying, replay, operations, troubleshooting),
`docs/reference/` (architecture, protocol, storage, ipc, query, replay,
adapters, mcp, cli), `docs/performance.md`, `docs/project/` (spec, plan) and
`docs/images/`. The roadmap is a table in the README. `xtask docs` enforces this
layout. Documentation describes what is implemented, not aspirations.

## 4. Core principle
Layers: Protocol, IPC, Data Plane, Runtime, Storage, Encoding, Index, Query,
State, Replay, Metadata, Adapters, MCP, CLI. The core knows Entity, Signal,
Stream, Event, Sample, Value, Timestamp, Sequence, Quality, Schema, Source,
Metadata, State. It must NOT know BACnet, AHU, Chiller, Modbus, OPC-UA, Robot,
Hospital, Building — those belong to adapters and metadata.

## 5. Operating systems
Linux, macOS and Windows from the start. Platform code sits behind Rust traits
(e.g. `ipc/{mod,unix,windows}.rs`); no `cfg` scattered through the application.

## 6. Process model
A standalone local daemon, `norn`. Applications connect over gRPC on local IPC
to the IPC layer, then the daemon (Runtime, Storage, Index, Query, Replay). No
network server is required for normal local operation.

## 7. IPC
A transport abstraction. Control plane: gRPC over local IPC. Data plane: shared
memory / memory-mapped ring buffer where beneficial. Fallback: regular IPC.
Evaluate Linux (Unix sockets, shared memory, memfd), macOS (Unix sockets,
shared memory, mmap), Windows (named pipes, shared memory, memory-mapped
files). The public protocol never depends on the transport; clients don't know
which one they use.

## 8. gRPC
Protobuf is canonical. Follow Google AIPs: resource-oriented, standard naming,
standard error model, pagination, field masks, resource names, long-running
operations, standard timestamps/durations/annotations. Use `buf` for lint,
breaking-change detection and codegen; pass AIP-oriented lint. Design a stable
external API — never RPCs that just expose internal Rust functions.

## 9. Protocol packages
`norn.type` (shared components, AIP-213; named `norn.common` in the original brief), `norn.entity`, `norn.signal`, `norn.stream`, `norn.telemetry`,
`norn.query`, `norn.replay`, `norn.state`, `norn.admin`. Services (cohesive, not
one giant service): NornService, TelemetryService, QueryService, ReplayService,
StateService, AdminService.

## 10. Data model
- **Entity**: a thing that exists (building, floor, AHU, motor, robot, sensor, vehicle).
- **Signal**: a measurable/observable property of an entity (temperature, pressure, speed, position, power, state, occupancy).
- **Stream**: a logical sequence of temporal observations.
- **Sample**: a timestamped observation — timestamp, signal, value, quality, sequence.
- **Value**: bool, int32, int64, uint32, uint64, float32, float64, string, bytes, enum; designed for extension.

## 11. BACnet as an example only
Entity `AHU-01`, Signal `SupplyAirTemperature`, metadata `protocol=BACnet,
device_instance=1001, object_type=analogInput, object_instance=3,
unit=degree_celsius`. Stored data is conceptually `timestamp, entity_id,
signal_id, value, quality, sequence`. Norn never needs to understand BACnet
object types; BACnet metadata stays in the signal/entity metadata layer.

## 12. Storage
A native Norn format — Parquet is NOT the primary internal representation.
Optimise for sequential writes, ingestion throughput, lossless compression,
temporal locality, range queries, replay, state reconstruction, crash
recovery, partial reads, efficient storage. Parquet and Arrow are
interoperability formats; MCAP is import/export or raw event-log integration.

## 13. Storage hierarchy
Database → Stream → Segment → Chunk → Block → Sample. A segment holds many
temporal chunks; each chunk is independently encoded, compressed, checksummed,
indexed and recoverable. No enormous monolithic files.

## 14. Compression
A pluggable encoding architecture, lossless, chosen by data characteristics.
Timestamps: regular interval, delta, delta-of-delta. Integers: delta, bit
packing, RLE. Booleans: bit packing, RLE. Enums: dictionary, RLE, bit packing.
Floats: XOR/Gorilla; evaluate ALP for decimal-like data. ZSTD as an optional
block compressor; never blindly recompress compressed data.
**Decision (owner, 2026-11-01):** ZSTD comes from a pure-Rust implementation,
not the C library, used at levels 3–6, chosen per block by measurement. The
codec piece confirms an existing pure-Rust encoder truly implements those
levels (not just decode, or only its fastest level). Its output must be
standard zstd frames, readable by any zstd decoder. If no pure-Rust encoder
reaches level 3, report back before substituting anything.

## 15. Expected workload
Benchmark model: 16,000 signals at 1 Hz for 365 days ≈ 504.6 billion
samples/year. Never assume the dataset fits in memory; every algorithm is
streaming or bounded-memory.

## 16. Memory
Explicit memory behaviour per subsystem. No unbounded queues, buffers, caches,
batch sizes, subscriptions, query results or replay buffers. Implement memory
budgets, backpressure, bounded channels, bounded caches, batch, query and
replay limits. Under memory pressure Norn keeps running and degrades through
backpressure, never uncontrolled allocation.

## 17. Peak load
Normal 16,000 samples/s; bursts of 100,000+ samples/s. Client batching, IPC
batching, lock-free/bounded queues where justified, per-stream buffering,
parallel encoding, sequential durable writes. Benchmark sustained and burst
separately.

## 18. Backpressure
Explicit outcomes: ACCEPT, BUFFER, THROTTLE, REJECT. Clients learn why data was
rejected or delayed. Never silently drop telemetry; dropping, if ever
supported, is an explicit policy.

## 19. Durability
Acknowledgement levels RECEIVED, BUFFERED, PERSISTED, DURABLE. DURABLE succeeds
only once the configured durability boundary is met. Document each exactly.

## 20. Crash recovery
Survive process crash, power loss, partial write, corrupted chunk, interrupted
index update, incomplete compression block, client disconnect, shared-memory
producer crash. Checksums, atomic metadata updates, recovery markers,
incomplete-segment detection, index rebuild, corruption reporting. Never
silently discard corrupted data.

## 21. Indexing
A temporal index supporting signal→chunks, time range→chunks, stream→chunks,
entity→signals, timestamp→nearest chunk. Sparse where possible; never per
sample unless benchmarks prove it necessary. The index is bounded and
recoverable.

## 22. Query
Range, Latest, First, Last, Count, Min, Max, Average, Sum, Rate. Filters:
entity, signal, time range, sampling interval, value predicates, quality
predicates. Skip irrelevant chunks; stream results, never build huge in-memory
datasets.

## 23. State reconstruction
`StateAt(timestamp)`: nearest checkpoint → load → replay changes → requested
timestamp → state. Domain-independent.

## 24. Checkpoints
Periodic state checkpoints, configurable interval; benchmark 1 min, 5 min,
15 min, 1 h and choose defaults from measurements.

## 25. Replay
Deterministic `Replay(start_time, end_time, filters, speed)` with pause,
resume, seek, step, stop; speeds 0x, 1x, 10x, 100x, maximum. Preserve
timestamp, sequence, ordering, signal identity and event semantics.

## 26. Scrubbing
`StateAt(T)` is first-class and fast enough for interactive use; callers never
deal with chunks.

## 27. Live and historical unification
Live and replayed telemetry share one event model and one subscription
abstraction (LIVE or REPLAY), so history behaves like a live system.

## 28. Replay clock
A virtual clock independent of wall time: current_time, speed, pause, resume,
seek, step — so applications and AI agents interact with history
deterministically.

## 29. Adapters
Outside the core, behind one adapter contract (BACnet, Modbus, OPC-UA, MQTT,
MCAP, Parquet, CSV, custom Protobuf), converting external data into Entity,
Signal, Sample, Metadata.
**Decision (owner, 2026-11-02):** the BACnet adapter lives in `bacnet-rs`, as a
client over IPC using only Norn's public crates, not under `adapters/` here
(docs/project/plan.md, "Connecting bacnet-rs").

## 30. BACnet adapter (later)
Device and object discovery, object metadata, property reads, subscriptions,
polling, quality, units, timestamps. A BACnet object maps to Entity + Signal +
Metadata; no BACnet structures in the core. (The `bacnet-rs` library supplies
the protocol side, and by the §29 decision the adapter itself.)

## 31. MCP
An optional, independent MCP server executable above Norn: AI agent → MCP
server → Norn gRPC client → daemon. It never touches storage directly. Tools:
list_entities, list_signals, get_signal_metadata, query_range, get_latest,
state_at, find_events, replay, compare_periods, get_statistics.

## 32. AI-oriented temporal operations (eventually)
What happened between T1 and T2; what changed; state at T; state before event
X; what happened just after X; anomalies in a period; compare with last week;
replay around an incident. No AI reasoning inside Norn — it supplies
deterministic temporal primitives.

## 33. CLI
`nornctl`: status, streams, entities, signals, inspect, query, state, replay,
verify, compact, export, stats, benchmark — through the same IPC API, never
bypassing internals.

## 34. Observability
Metrics: ingestion_rate, persisted_rate, bytes_written, bytes_read,
compression_ratio, memory_usage, queue_depth, backpressure_events,
ipc_latency, query_latency, replay_rate, checkpoint_count, active_clients,
storage_size, index_size, recovery_events, corruption_events. Structured
logging. Observability itself is bounded.

## 35. Performance
Benchmarks: IPC latency, gRPC latency, shared-memory throughput, samples/s,
batch throughput, disk throughput, (de)compression throughput, query latency,
StateAt latency, replay throughput, memory, CPU. At 1,000 / 10,000 / 16,000 /
100,000 signals and 1 / 10 / 100 Hz, with realistic sensor distributions.

## 36. Correctness
Unit, integration, property-based, fuzz, crash, corruption, concurrency and
cross-platform tests. Invariants: `decode(encode(data)) == data`;
record → store → replay yields equivalent events; `StateAt(T)` equals the
state from replaying every relevant event up to T.

## 37. CI
Linux, macOS, Windows: compile, unit and integration tests, protobuf
generation, buf lint, formatting, clippy, docs, benchmarks where appropriate.
`cargo fmt`, `cargo clippy`, `cargo test`, `cargo doc`, `buf lint`,
`buf breaking`.

## 38. Repository structure
A workspace: `README.md`, `CONTRIBUTING.md` and `SECURITY.md` at the root, and
`docs/` as section 3 lays it out; `protobuf/`; `crates/` {core, protocol, ipc,
transport, shared-memory, runtime, storage, codec, index, query, state, replay,
metadata, server, cli}; `adapters/` {bacnet, modbus, opcua, mqtt, mcap,
parquet}; `services/mcp/`; `tests/`; `benchmarks/`; `examples/`; `tools/`
(`tools/screenshots/` captures `docs/images/` from a real daemon). One focused responsibility per crate/module.

## 39. Dependencies
Minimise. Before adding one: identify the problem, check std, evaluate
maintenance, licence, security history, cross-platform support and
performance, and document why.

## 40. API stability
Version the protocol; never silently break stored data. Storage carries magic,
version, schema_version, codec_version. Capability negotiation; old clients
fail clearly rather than behave incorrectly.

## 41. Security
Protect IPC endpoints, files, shared memory and resources with OS permissions.
Prevent clients exhausting memory or creating unlimited streams, metadata,
unbounded queries or replay sessions.

## 42. Compaction
Merge small chunks, optimise compression, rebuild indexes, preserve data,
crash-safe, without blocking ingestion unnecessarily.

## 43. Retention
Optional and explicit: forever, N days, N months, until a size limit. Never
delete data silently.

## 44–45. Export and import
Export Norn → Parquet, Arrow, CSV, MCAP through the public query/read
abstractions. Import Parquet, MCAP, CSV → Norn, streaming and bounded-memory.

## 46. Local first
Fully offline: no cloud, Kubernetes, external database, object storage or
network needed for core functionality.

## 47. Integration rule
External System → Adapter → Norn Protocol → Norn. Never into the internal
storage API; storage stays replaceable.

## 48. Implementation order
1 Foundation (repo, workspace, docs, core types, error model, config) ·
2 Protocol (Protobuf, gRPC, Buf, AIP lint, versioning) · 3 IPC (Unix sockets,
Windows named pipes, transport abstraction, framing, connections) · 4 Basic
daemon (startup, shutdown, config, lifecycle, health) · 5 Storage (format,
chunks, segments, checksums, recovery) · 6 Encoding (timestamp, integer,
float, boolean, enum, ZSTD) · 7 Index · 8 Write path (append, batch,
buffering, backpressure, durability) · 9 Query (range, latest, aggregates) ·
10 State (checkpoints, StateAt) · 11 Replay (pause, seek, speed, virtual
clock) · 12 Shared memory (ring buffer, fallback) · 13 Adapters (BACnet, MCAP,
Parquet, others) · 14 MCP · 15 Optimisation · 16 Hardening (fuzzing, crash
recovery, corruption, cross-platform, security review).

## 49. Definition of done
`cargo test`, `cargo clippy`, `cargo fmt --check`, `cargo doc`, `buf lint`,
`buf breaking` pass on Linux, macOS and Windows. Demonstrated: high-throughput
ingestion, bounded memory, crash recovery, lossless compression, range
queries, StateAt, deterministic replay, interactive scrubbing, IPC. Benchmarks
at 16,000 signals × 1 Hz with an extrapolated estimate for 504+ billion
samples/year.

## 50. Final principle
Optimise Norn around **time**, not BACnet. Input → STORE, INDEX, QUERY, STATE,
REPLAY, SCRUB, STREAM → humans, systems, AI.

> **Given any point in time, Norn should make it possible to understand what
> the system knew, what changed, and what happened around that moment.**

Build incrementally without violating these boundaries. Before each major
subsystem, inspect the existing architecture and docs, identify conflicts with
this specification, and update the design docs before the code. Never a large
file to solve a problem quickly. Prefer small modules, explicit interfaces,
determinism, bounded memory and measurable performance. When ambiguous,
choose in this order: portability, correctness, protocol stability, data
durability, bounded resources, extensibility — and document the decision.
