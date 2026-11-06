<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# MCP

Implemented: MCP inside the `norn` daemon (SPEC §31, §32, stage 14). An AI agent reads
Norn's history through the [Model Context Protocol](https://modelcontextprotocol.io),
and every tool is an RPC of the public schema that any gRPC client can call too. The
how-to, with a client configuration, is the [AI agents guide](../guides/ai-agents.md).
This page is the design, the tool list and every bound.

```text
AI agent ──MCP (stdio, local socket, loopback HTTP)──▶ norn ──in-process──▶ gRPC services
```

## RPC first, generated MCP

Nothing about a tool is written by hand. Each read method in `protobuf/` carries
`(mcp.v1.tool)` with `read_only: true`, its service `(mcp.v1.service)` with a private
cache scope, and some arguments `(mcp.v1.field)` examples that help a model.
[protoc-gen-mcp](https://github.com/the-protobuf-project/mcp) v1.7.8 generates one
handler per service from them (`buf.gen.mcp.yaml`, into
`crates/server/src/mcp/generated/`, committed and checked by `just gen-check`): each
tool's name, title, description (the RPC's comment), input schema and hints.

A tool's arguments are its RPC's request message as canonical protobuf JSON, and its
result is the response message as JSON. The hand-written part is a thin bridge
(`crates/server/src/mcp/`):

| File | Does |
| --- | --- |
| `bridge.rs` | implements every generated trait once: a read is its RPC by name; every other method's arm is a refusal |
| `rpc.rs` | JSON to the request message and back, by the embedded descriptor set (`norn_protocol::FILE_DESCRIPTOR_SET`), and one unary call through the loopback |
| `endpoint.rs` | combines the handlers; lists and dispatches only tools whose `readOnlyHint` is true |
| `sessions.rs` | the replay sessions MCP opened, and their bound |
| `transport.rs` | stdio, and streamable HTTP on loopback TCP, a Unix socket or a named pipe, through runtime-rs's `tpp-agents` |

**Read-only by construction.** protoc-gen-mcp makes a tool of every unary RPC,
annotated or not. The endpoint serves only those annotated read-only, and the bridge
has no path to the daemon for any other method, so a write cannot be reached even by
calling its name. `crates/server/tests/mcp_surface.rs` lists the tools from a real
daemon and requires exactly the seventeen below, and calls writes by name to see them
refused.

A write may still carry `(mcp.v1.tool)` hints, to say what it is rather than to
expose it. `BatchCreateEntities` and `BatchCreateSignals` (protocol 1.6) are annotated
`read_only: false`, `destructive: false`, `idempotent: true`: the endpoint leaves them
out, the bridge refuses them, and `mcp_surface.rs` calls both by name.

**The same validation.** A tool call reaches the gRPC services through runtime-rs's
in-process `Loopback`: the routes behind the same layers a socket client passes, the
version gate, metered decoding against `ipc_bytes`, and the gate that holds calls until
the daemon runs. Nothing is dialled. MCP and gRPC therefore cannot disagree, and a daemon
restart takes MCP with it rather than leaving it to reconnect.

## Tools

| Tool | RPC | Answers |
| --- | --- | --- |
| `entity_service-list_entities_v1`, `-get_entity_v1` | `ListEntities`, `GetEntity` | what exists |
| `signal_service-list_signals_v1`, `-get_signal_v1` | `ListSignals`, `GetSignal` | what each entity measures, and a signal's metadata |
| `stream_service-list_streams_v1`, `-get_stream_v1` | `ListStreams`, `GetStream` | streams and their totals |
| `admin_service-get_server_v1` | `GetServer` | the daemon, its limits and recovery |
| `query_service-browse_samples_v1` | `BrowseSamples` | history over an interval, raw or aggregated; `LATEST` for the newest sample |
| `query_service-find_events_v1` | `FindEvents` | when a predicate held, a value or a quality changed |
| `query_service-compute_stats_v1` | `ComputeStats` | count, min, max, average, sum, first and last per signal |
| `query_service-compare_periods_v1` | `ComparePeriods` | those for two intervals, and the change |
| `state_service-compute_state_v1` | `ComputeState` | what every signal held at an instant |
| `replay_service-create_replay_session_v1` | `CreateReplaySession` | a session to drive; create it with `speed: 0` |
| `replay_service-drive_replay_session_v1` | `DriveReplaySession` | steps or seeks, returning what that emitted |
| `replay_service-get_replay_session_v1`, `-list_replay_sessions_v1` | `GetReplaySession`, `ListReplaySessions` | sessions and their clocks |
| `replay_service-delete_replay_session_v1` | `DeleteReplaySession` | ends a session; always do this when finished |

[protocol.md](protocol.md#bounded-unary-reads) gives the exact semantics of the query
and replay reads: bit-exact change detection, a signal's first sample as the baseline,
page tokens that resume exactly, every page bound, values cut and flagged.

The replay tools are read-only too: a session is a view of stored history, held in
memory, and creating, driving or deleting one changes nothing stored. The controls that
pace a session for its subscribers (`Pause`, `Resume`, `Seek`, `Step`, `Stop`, `Update`)
have no tool. `QuerySamples` and `SubscribeEvents` are streams, which protoc-gen-mcp does
not make tools of; `BrowseSamples` and `DriveReplaySession` are their unary forms.

**Sessions.** A tool names only sessions a tool created: a read-only surface must not
stop or move a replay someone else is watching. `ListReplaySessions` shows only those.
At most 8 are open at once (the daemon's own limit, 16 by default, is shared with gRPC
clients); a create past it first forgets sessions the daemon has expired, then fails with
`MCP_LIMIT_EXCEEDED`. An undeleted session expires after 10 idle minutes.

## Running it

MCP is off unless asked for; gRPC on the local endpoint is always on.

```toml
[mcp]
enabled = true
# Any of: "stdio", "http:127.0.0.1:8090", "unix:/run/user/1000/norn/mcp.sock",
# "pipe:\\.\pipe\norn-mcp". Enabled with none named serves stdio.
transports = ["stdio"]
# Further Host names HTTP answers, such as a tunnel's.
hosts = []
```

Or `norn --mcp stdio`, repeatable, which enables it. With stdio, the agent's client
launches `norn` itself, and the client closing stdin stops the daemon cleanly. Logs go to
stderr, never stdout. HTTP is at `/mcp`.

- **No TCP unless named.** A default daemon, and one serving MCP on stdio or a socket,
  opens no TCP port; `mcp_surface.rs` checks the process's sockets with `lsof`.
- **Loopback only, and open to every local user.** `http:` with a non-loopback address
  is refused at startup. There is no authentication: once MCP over HTTP is enabled, any
  program of any user on the machine can call every tool and read all history, and the
  daemon only logs a warning at every start. This is kept by the owner's decision
  ([security review](../security/review-2026-11.md#owner-decision-f4-and-f5), F5). Where
  other users share the machine, use `stdio`, a `unix:` socket or a `pipe:` name
  instead. To reach it from elsewhere, put a tunnel in front and name it in `hosts`.
- **A `Host` allow-list.** `localhost`, `127.0.0.1` and `::1`, any port, plus `hosts`;
  any other `Host` gets 403, which defeats DNS rebinding. tpp-agents enforces it.
- **Owner-only sockets and pipes.** A Unix socket is created 0600 in a 0700 directory, a
  stale one replaced, a live one or a regular file refused, and removed on stop. The
  daemon checks every MCP socket's directory at start as it does the gRPC one. A named
  pipe is created by runtime-rs with an owner-only security descriptor.

## Bounds

| Bound | Value |
| --- | --- |
| Tool calls at once | 8 across every client; the rest wait |
| Deadline per call | 30 s, then `MCP_DEADLINE_EXCEEDED` (retryable) |
| Replay sessions MCP holds | 8 |
| Pages, values, scans | the RPCs' own: [protocol.md](protocol.md#bounded-unary-reads) |

## Errors

A failed call is a tool result with `isError: true`, so the model reads it. Its text is
`{"error": {"reason", "message", "retryable", "metadata"}}`. `reason` is the daemon's
`ErrorInfo` reason (`ERROR_REASON_*`, protocol.md's error model), `GRPC_<CODE>` for a
status without one, or this layer's own: `MCP_LIMIT_EXCEEDED`, `MCP_UNKNOWN_SESSION`,
`MCP_DEADLINE_EXCEEDED`. `retryable` is true for `UNAVAILABLE`, `RESOURCE_EXHAUSTED` and
`ABORTED`. An unknown tool name, a write's name included, is a protocol error.

## Dependencies

| Crate | Why |
| --- | --- |
| `tpp-grpc`, `tpp-agents` (runtime-rs) | the `HybridServer` and MCP's transports; built without TLS, so no ring; they bring OpenTelemetry, which `xtask deps` excuses beneath `norn-server` only ([architecture](architecture.md#no-c)) |
| `rmcp` 3.8 | the SDK the generated handlers are written against |
| `prost-reflect` | JSON to messages by descriptor; already tpp-grpc's |
| `async-trait`, `axum`, `serde_json` | named by the generated code; already in the graph |

## Tests

Against the real `norn` binary and rmcp's real client:

| File | Proves |
| --- | --- |
| `mcp_tools.rs` | every tool, over stdio and over a Unix socket, equals a brute-force answer from the seeded data |
| `mcp_surface.rs` | the tool list is exactly the reads and a write's name is refused; no TCP port by default or with stdio and a socket; the socket is 0600; a stdio client leaving stops the daemon cleanly; another client's session is out of reach and MCP's are bounded; a non-loopback address is refused |
| `browse.rs`, `find_events.rs`, `find_events_bits.rs`, `stats.rs`, `drive.rs` | the RPCs themselves over gRPC, at every page size, against brute force |

## Not implemented

- Writes, by decision: SPEC §31 lists only reading tools.
- Live subscriptions: an agent polls `BrowseSamples` with `LATEST`, or `ComputeState`.
- Resources and prompts; tools only.
- Authentication on HTTP, which is why it binds loopback only.
- Protocol revision `2026-07-28`, held back deliberately: rmcp's results
  lack fields that revision requires, and strict clients reject them.
- Relative times (`now-1h`): arguments are the schema's RFC 3339 times.
- REST: the schema's HTTP bindings could be transcoded by the same server, but that
  needs a route table generated with protoc-gen-http; Norn serves gRPC and MCP only.
