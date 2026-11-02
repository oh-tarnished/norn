# MCP

Implemented: `services/mcp` (`norn-mcp`, SPEC §31, §32, stage 14), an independent
executable that gives AI agents Norn's temporal primitives as
[Model Context Protocol](https://modelcontextprotocol.io) tools. The how-to, with a real
session and a client configuration, is the [AI agents guide](../guides/ai-agents.md).
This page is the design, the tools' exact semantics and every bound.

```text
AI agent ──MCP (stdio or loopback HTTP)──▶ norn-mcp ──gRPC over local IPC──▶ norn
```

## Layering

| | |
| --- | --- |
| Owns | the ten tools, their schemas and descriptions, bounded results, continuations, the two transports |
| Imports | `norn-protocol` and `norn-ipc` only ([architecture](architecture.md)); never storage, the runtime or the server |
| Must not | write to Norn; reason. Every tool is a deterministic primitive (SPEC §32): nothing is inferred, ranked or summarised in words |
| Creates in the daemon | replay sessions only, and it stops them itself |

Every tool call is one or more ordinary gRPC calls, each stamped with
`norn-protocol-version: 1.3`, so a daemon of another major fails the call clearly
rather than being misread. `norn-mcp` connects lazily: it starts without a daemon,
lists its tools without one, and a call made while the daemon is down, restarting or
gone fails with `DAEMON_UNAVAILABLE` and then succeeds once it is back. Nothing about
a daemon is cached between calls.

## Running it

```text
norn-mcp [--endpoint ENDPOINT] [--mcp stdio|http:ADDRESS:PORT] [--mcp-host NAME]... [--timeout SECONDS]
```

| Flag | Default | Meaning |
| --- | --- | --- |
| `--endpoint` | the platform's per-user endpoint, as `norn`'s | the daemon |
| `--mcp` | `stdio` | `stdio`, one client on stdin/stdout; or `http:127.0.0.1:8090`, streamable HTTP at `/mcp` |
| `--mcp-host` | none | with HTTP, a further `Host` name to answer, such as a tunnel's. Repeatable |
| `--timeout` | 30 | the deadline for one tool call, every daemon call in it included |

Exit status: 0 when the stdio client leaves or `SIGTERM`/Ctrl-C arrives, 1 when serving
failed (an address in use), 2 for a malformed command line. Logs go to stderr, never
stdout, which stdio carries; a closed stderr is tolerated.

On the way out it stops every replay session it opened, then exits. A `kill -9` cannot
do that, and the daemon then removes the sessions after its idle timeout (10 minutes).

## HTTP and its guards

`norn-mcp` has no authentication, so its HTTP transport is for this machine only,
following bacnet-rs's `--mcp-host` practice:

- **Loopback addresses only.** `--mcp http:0.0.0.0:8090` or any other non-loopback
  address is refused at the command line with exit status 2. To reach it from
  elsewhere, put a tunnel in front and name the tunnel with `--mcp-host`.
- **A `Host` allow-list.** `localhost`, `127.0.0.1` and `::1`, on any port, plus each
  `--mcp-host`. Anything else gets `403 Forbidden` before a byte is parsed as MCP. This
  is what defeats DNS rebinding, where a web page gets a loopback address under its own
  name.
- **An `Origin` allow-list.** A request carrying a browser `Origin` must come from
  `http` or `https` on one of those same names, any port; another gets 403. A request
  without one, as every non-browser client sends, passes.
- Only `/mcp` is served; any other path is 404.

The `Host` and `Origin` checks are rmcp's, configured with these lists;
`services/mcp/tests/http.rs` proves each against the running binary.

## Tools

Ten tools, every one read-only and marked so (`readOnlyHint`, not destructive,
closed-world). Each input schema is derived from the Rust type its arguments parse into,
refuses unknown arguments (`additionalProperties: false`), and documents each argument
for a model. `replay` is the only tool not marked idempotent.

| Tool | Answers | Built on |
| --- | --- | --- |
| `list_entities` | what exists | `ListEntities` |
| `list_signals` | what each entity measures: type, unit, stream, annotations | `ListSignals` |
| `get_signal_metadata` | one signal's metadata and timestamps | `GetSignal` |
| `query_range` | history over an interval, raw or aggregated over windows | `QuerySamples` |
| `get_latest` | the newest sample of each signal | `QuerySamples`, `LATEST` |
| `state_at` | what every signal held at an instant | `ComputeState` |
| `find_events` | when something happened: a predicate held, a value or quality changed | `QuerySamples` |
| `get_statistics` | count, min, max, first, last, average, sum per signal | `QuerySamples` aggregates |
| `compare_periods` | those statistics for two intervals, and the change | `QuerySamples` aggregates |
| `replay` | history step by step on a virtual clock | `ReplayService`, `SubscribeEvents` |

### Common arguments

- `target`: `all`, an entity (`entities/ahu-01` or `ahu-01`), or a signal
  (`entities/ahu-01/signals/supply-temp`). Entity and signal IDs are checked
  (`[a-z0-9-]`) before they are quoted into a filter, so a target cannot rewrite one.
- `select`: a further signal filter, as `ListSignals` takes ([protocol](protocol.md)).
- `from`, `to`: a half-open interval. RFC 3339 in UTC (`2026-11-02T08:00:00Z`), Unix
  seconds (`1793606400`, fraction allowed), `now`, or `now-1h`. Either may be unset.
  `now` is fixed by a read's first call and carried in its continuation, so
  `now-1h` names one interval across every page.
- `where`: the sample filter of [query.md](query.md#sample-filter), over `value`,
  `quality.severity` and `quality.substatus`.
- `limit` and `continuation`: below.

### Values in results

A row of samples is `{signal, time, value, quality, sequence}`. `time` is RFC 3339 in
UTC with only the fraction it needs. Numbers stay JSON numbers; a float is shown by its
shortest exact text, so a `float` of 0.1 is `0.1`. JSON has no NaN or infinities, so they
are the strings `"NaN"`, `"Infinity"` and `"-Infinity"`. A `uint64` is its unsigned
value. Bytes are hex. An enum is its number, or `{number, symbol}` when it has a symbol.
`quality` is `GOOD`, `BAD/SENSOR_FAILURE` and so on.

A string or bytes value longer than 1,024 characters (as JSON escapes it) is cut, and
its row carries `truncated: true`: never a silent loss, and no one sample can make a
result unbounded.

### query_range

Raw samples, or with `aggregate` (`first`, `last`, `count`, `min`, `max`, `average`,
`sum`, `rate`) one row per signal and `window` (`500ms` ... `1d`), aligned to `from`.
`order` is `signal` (default, cheaper) or `time` (replay order); aggregates are always
by signal. Every rule is the daemon's ([query.md](query.md#semantics)): a damaged chunk
is an error, never a gap, and `rate` is a counter's, so it is wrong on a gauge. An
aggregate row is `{signal, start, end, value, sample_count}`.

### get_latest

`LATEST` over all history: `{signal, time, value}` per signal with a sample. `where`
asks for the newest matching sample, such as the newest `GOOD` one. A signal with no
matching sample has no row; `state_at` gives every signal a row.

### state_at

`ComputeState` at `at`: every selected signal has a row, in name order, `known: false`
when it had no sample by then or, with `max_age`, none recent enough. A sample at
exactly `at` counts. The answer is pinned in the daemon, so pages agree.

### find_events

An event is exactly one of three things, chosen by `kind`; nothing is inferred:

| `kind` | A row for each sample that | Needs |
| --- | --- | --- |
| `match` | passes `where` | `where` |
| `value_change` | has a value differing, bit for bit, from the signal's previous sample | |
| `quality_change` | has a quality (severity and substatus) differing from the previous sample's | |

- "Previous" is the previous sample of the same signal within the interval, in
  `(time, sequence)` order, among samples passing `where` when it is given. A signal's
  first sample in the interval is the baseline and never a change: widen `from` to see
  one at the start.
- Bit for bit: `-0.0` differs from `0.0`, a NaN equals the same NaN, and an enum compares
  by number, not symbol. A value cut for display is still compared whole.
- Rows carry `event` and, for a change, `previous`. They come grouped by signal, in
  time order, because reading in storage order needs only one previous sample in memory
  whatever the selection.
- Domain words such as "alarm" or "anomaly" are the caller's: an agent builds them from
  these, such as `match` with `value > 80` or `quality.severity = QUALITY_SEVERITY_BAD`.

### get_statistics and compare_periods

`get_statistics` gives, per signal over the interval: `sample_count`; `min` and `max`
(any type that orders); `average` and `sum` (numeric types only); `first` and `last`,
each `{time, value}`. They come from the daemon's `COUNT`, `MIN`, `MAX`, `AVERAGE`, `SUM`
and `LATEST`, plus the first page of a raw read for `first`'s time. No sample but those
two crosses to `norn-mcp`. A signal with no samples has only `sample_count: 0`.

`compare_periods` is that for two closed periods `a` and `b`, plus `change`: b minus a
for `sample_count`, `min`, `max`, `average` and `sum` where both are numbers, in IEEE
arithmetic.

These are several reads, not one snapshot: on a signal still receiving samples, an
interval that reaches `now` can see a sample in one aggregate and not another. Name a
closed interval in the past when that matters.

### replay

One tool with an `action`, so an agent drives a session one bounded call at a time:

| Action | Needs | Does |
| --- | --- | --- |
| `start` | `target`, `from`, `to`, optional `select` | creates a session paused at `from`, frozen (speed 0): nothing happens until you step |
| `step` | `session`, `events` or `duration` | emits the next `events` samples, or every sample in the next `duration` of virtual time |
| `seek` | `session`, `time` | moves the clock; returns each signal's state just before `time` (`event: "state"`) |
| `status` | `session` | the session: `state`, `clock`, `samples_emitted`, `expires` |
| `stop` | `session` | ends and deletes it. Always do this when finished |
| `list` | | the sessions this server has open |

A `step` or `seek` returns the session with `events` (rows of samples tagged `sample` or
`state`), `ended` (the clock reached `to`; seek back or stop), and `omitted`. The events
are exactly what `query_range` with `order: time` gives for the same stretch, because
both are the daemon's replay order.

How each call sees everything it caused: it subscribes, applies the control, then sets
the pace to 0 again, which the daemon answers with a `SPEED_CHANGED` event after every
event the control emitted. Reading up to that event is reading all of it, with no
timing involved.

Bounds: `events` above `limit` is refused before anything moves, and so is a `duration`
whose samples, counted first with the session's own selection, exceed it. A seek's state
rows are one per signal and can pass `limit`; the excess is counted in `omitted`, never
hidden.

Sessions: at most 8 open per `norn-mcp` process (the daemon's own limit, 16 by default,
is shared with other clients). A `start` past it first forgets sessions the daemon has
expired, then fails with `MCP_LIMIT_EXCEEDED`. A session can be named only by the server
that opened it: a read-only surface must not stop a replay someone else is watching.
Sessions not stopped are stopped when `norn-mcp` exits, and expire in the daemon after 10
idle minutes otherwise.

## Bounded results and continuations

Every tool that lists rows returns `{rows, count, complete, continuation}`. `complete` is
false exactly when `continuation` is a string; repeat the call with the same arguments
plus that `continuation` to read on. Only `limit` may change between pages.

| Bound | Value |
| --- | --- |
| `limit` default, maximum | 100, 1,000 rows (`list_*`, `query_range`, `get_latest`, `find_events`, `replay` events) |
| `state_at` `limit` | 100, at most 180: a whole daemon page must fit one result |
| `get_statistics`, `compare_periods` `limit` | 20, at most 50 signals: each is six reads, twelve for a comparison |
| JSON per result | 256 KiB of rows; the first row always fits, so a call always moves forward |
| One value | 1,024 characters, then `truncated` |
| Samples `find_events` scans per call | 1,000,000; past it the call returns with a continuation, possibly with no rows |
| Deadline per call | `--timeout`, 30 s; a read that reaches it with rows returns them and a continuation |
| Tool calls at once | 8, across all clients; the rest wait within their deadlines |
| HTTP connections | 64; request bodies 4 MiB (rmcp's) |

Memory is one daemon page (at most 10,000 samples, about 2 MiB) plus one result per
call in flight. Every value above was picked, not measured.

**What a continuation holds.** The daemon's resume token for the page the result
stopped in, the position (signal, time, sequence) of the last row returned, the meaning
of `now`, a fingerprint of the arguments, and for `find_events` the last sample's value
or quality. The next call re-reads that page and skips what was given, so nothing is lost
or repeated even though a result stops inside a daemon page. A position, not a count, so
a sample arriving between calls cannot shift it. It is hex of JSON: opaque, not secret.
`norn-mcp` keeps nothing between calls, so a continuation survives a new MCP session and a
restart of `norn-mcp`, and a query's survives a daemon restart too. `state_at`'s is the
daemon's pinned-answer token, valid for about a minute.

`services/mcp/tests/bounds.rs` reads 1,100 samples in both orders at limits 1, 7, 333 and
1,000 and requires identical rows.

## Errors

A failed call is a tool result with `isError: true`, never a protocol error, so the model
reads it. Its text is one JSON object:

```json
{"error": {"reason": "...", "message": "...", "hint": "...", "retryable": false, "metadata": {}}}
```

Branch on `reason`; `message` is for reading. An unknown tool name is the one protocol
error (`invalid params`).

| `reason` | When | `retryable` |
| --- | --- | --- |
| `DAEMON_UNAVAILABLE` | no daemon at the endpoint, or the connection dropped | yes |
| `ERROR_REASON_*` | the daemon's own reason, with its `ErrorInfo` metadata ([protocol](protocol.md#error-model)) | for `UNAVAILABLE`, `RESOURCE_EXHAUSTED`, `ABORTED` |
| `GRPC_*` | a daemon status with no reason, such as from a much older daemon | as above |
| `MCP_INVALID_ARGUMENT` | an argument this server refuses before asking the daemon; the message names it | no |
| `MCP_INVALID_CONTINUATION` | malformed, or issued for other arguments | no |
| `MCP_LIMIT_EXCEEDED` | a bound above; `metadata.limit` names it | no |
| `MCP_DEADLINE_EXCEEDED` | the call reached `--timeout` with nothing to return | yes |
| `MCP_UNKNOWN_SESSION` | a replay session this server did not open, or has stopped | no |

`DATA_LOSS` keeps the daemon's metadata (`chunk`, `signal_id`, `min_time`, `max_time`), and
its hint names the instants to read around in RFC 3339. A refused daemon field is named in
the hint as the tool argument a model typed: `sample_filter` is `where`.

## Dependencies

| Crate | Why | Graph |
| --- | --- | --- |
| `rmcp` 3.8, features `server`, `transport-io`, `transport-streamable-http-server` | the MCP SDK the owner's other servers use; no default features | pure Rust, no TLS |
| `schemars` 1 | input schemas derived from the argument types | already rmcp's |
| `hyper`, `hyper-util`, `http-body-util`, `tower`, `http`, `bytes` | serving one route without a web framework | already tonic's |
| `serde`, `serde_json`, `prost` | arguments, results, hashing values | already the workspace's |
| `tracing-subscriber` | rmcp's warnings onto stderr | already the daemon's |

Not runtime-rs's `tpp-agents`: it brings aws-lc and OpenTelemetry, which the no-C rule
(SPEC §1) and the dependency rules forbid. The tests use rmcp's client over a child
process and over HTTP with `reqwest`, with no TLS feature; dev-only, so it never ships.
Every licence in the graph passes `deny.toml`.

`schemars` depends on `chrono` with its default features, whose dependency lists name
`iana-time-zone-haiku` (`cc`, on Haiku only) and `wasm-bindgen-shared` (a `links` marker,
on wasm32 only). Neither is compiled for any target Norn builds, so `xtask deps` now
examines the graph Cargo resolves for those targets, listed in `deps::PLATFORMS`, rather
than every platform's ([architecture](architecture.md#no-c)). Its tests prove a
Windows-only dependency is still examined and a Haiku-only one is not.

## Tests

Each runs a real daemon, the real `norn-mcp` binary and rmcp's real client:

| File | Proves |
| --- | --- |
| `tools.rs` | every read tool over stdio and over HTTP equals a brute-force answer from the seeded data |
| `compute.rs` | `find_events` (all three kinds, at several limits), statistics and comparisons equal hand computation |
| `replay.rs` | steps, seeks, the refusal of a step too big, ending, stopping, the session bound, another client's session out of reach |
| `lifecycle.rs` | a stdio client leaving, and `SIGTERM` to an HTTP server, both stop its sessions |
| `bounds.rs` | every page size reads the same rows, defaults are bounded, a long value is cut, a continuation serves only its arguments |
| `failures.rs` | `DATA_LOSS` from a chunk damaged on disk, bad input of every kind, a daemon down at start, going away and coming back |
| `http.rs` | a forbidden `Host` and `Origin` get 403, a named host is answered, only `/mcp` is served, non-loopback is refused |

## Not implemented

- Writes. The surface is read-only in v0.1, as SPEC §31 lists only reading tools.
- Live subscriptions: an agent polls `get_latest` or `state_at` instead.
- MCP resources and prompts; tools only.
- Authentication on HTTP, which is why it binds loopback only.
- Protocol revision `2026-07-28`, held back as bacnet-rs holds it back: rmcp's results
  lack fields that revision requires, and strict clients reject them.
