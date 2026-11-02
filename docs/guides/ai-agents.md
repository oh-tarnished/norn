# AI agents

`norn-mcp` lets an AI agent read Norn's history through the Model Context Protocol:
what exists, what happened over an interval, what everything held at an instant, when
something changed, and a step-by-step replay. It is read-only and reaches the daemon only
through the public gRPC API. Every tool is a deterministic primitive; the reasoning is
the agent's. The exact semantics and every bound are in the [MCP reference](../reference/mcp.md).

## Build and start

```sh
cargo build -p norn-server -p norn-cli -p norn-mcp
./target/debug/norn
```

Or take `norn-mcp` from a release archive: every platform's archive carries it beside
`norn` and `nornctl` (`norn-mcp.exe` on Windows).

An agent's client launches `norn-mcp` itself over stdio, so there is nothing else to
start. It finds the daemon at the same per-user endpoint `norn` and `nornctl` use;
`--endpoint` names another.

```console
$ norn-mcp --help
usage: norn-mcp [OPTIONS]

An MCP server over a Norn daemon. Read-only; it never touches storage.

options:
  --endpoint ENDPOINT  the daemon's endpoint, such as unix:/run/norn/norn.sock
                       (default: the platform's per-user endpoint, as norn's)
  --mcp TRANSPORT      stdio, or http:127.0.0.1:8090 for streamable HTTP at /mcp
                       on a loopback address (default: stdio)
  --mcp-host NAME      with http: also answer to this Host, such as a tunnel's.
                       Repeatable. Loopback names are always answered
  --timeout SECONDS    bound on each tool call (default 30)
  -h, --help           print this text
  -V, --version        print the version
```

## Configure a client

Most MCP clients, Claude Code and Claude Desktop among them, take a server entry like
this. Use absolute paths; `--endpoint` can go when the daemon uses its default.

```json
{
  "mcpServers": {
    "norn": {
      "command": "/path/to/norn/target/debug/norn-mcp",
      "args": ["--endpoint", "unix:/path/to/norn.sock"]
    }
  }
}
```

With Claude Code, `claude mcp add norn -- /path/to/norn-mcp` does the same. For a client
that connects to a running server instead, serve streamable HTTP on loopback and point it
at `http://127.0.0.1:8090/mcp`:

```console
$ norn-mcp --mcp http:127.0.0.1:8090
norn-mcp: serving http://127.0.0.1:8090/mcp
```

It binds loopback addresses only, answers only loopback `Host` names (plus any
`--mcp-host`), and refuses anything else with 403, so a web page cannot reach it by
rebinding a name to 127.0.0.1:

```console
$ curl -si -H 'Host: evil.example.net' -H 'Content-Type: application/json' \
       -H 'Accept: application/json, text/event-stream' -d '{}' http://127.0.0.1:8090/mcp
HTTP/1.1 403 Forbidden
...
Forbidden: Host header is not allowed
```

## A session

Below is real output: a fresh daemon, seeded with `nornctl` with six minutes of an air
handler's supply temperature and fan state, then the tool calls an agent would make, each
result exactly as `norn-mcp` returned it. Some rows are trimmed where marked.

```console
$ nornctl streams create plant
$ nornctl entities create ahu-01
$ nornctl signals create ahu-01 supply-temp plant double
$ nornctl signals create ahu-01 fan-on plant bool
$ nornctl append plant entities/ahu-01/signals/supply-temp 21.0 --at 2026-11-02T08:00:00Z --sequence 0
...one sample a minute: 21.0 21.5 22.4 23.1 22.0 21.2, and fan-on false true true true false false
```

The tool list:

```text
list_entities, list_signals, get_signal_metadata, query_range, get_latest, state_at,
find_events, get_statistics, compare_periods, replay
```

**When did the fan change?** `find_events` with `kind: value_change`. The first sample is
the baseline, not a change, and each change says what it was before:

```json
// find_events {"target": "entities/ahu-01/signals/fan-on", "kind": "value_change"}
{
 "complete": true,
 "continuation": null,
 "count": 2,
 "rows": [
  {"event": "value_change", "previous": false, "quality": "GOOD", "sequence": 1,
   "signal": "entities/ahu-01/signals/fan-on", "time": "2026-11-02T08:01:00Z", "value": true},
  {"event": "value_change", "previous": true, "quality": "GOOD", "sequence": 4,
   "signal": "entities/ahu-01/signals/fan-on", "time": "2026-11-02T08:04:00Z", "value": false}
 ],
 "samples_scanned": 6
}
```

**What did it look like just before the fan stopped?** `state_at`:

```json
// state_at {"target": "ahu-01", "at": "2026-11-02T08:03:30Z"}
{
 "complete": true, "continuation": null, "count": 2,
 "rows": [
  {"known": true, "quality": "GOOD", "sequence": 3, "signal": "entities/ahu-01/signals/fan-on",
   "time": "2026-11-02T08:03:00Z", "value": true},
  {"known": true, "quality": "GOOD", "sequence": 3, "signal": "entities/ahu-01/signals/supply-temp",
   "time": "2026-11-02T08:03:00Z", "value": 23.1}
 ],
 "state_time": "2026-11-02T08:03:30Z"
}
```

**Results are bounded.** With `limit: 3`, `query_range` returns three rows, `complete:
false` and a continuation; the same call with `"continuation": "7b22..."` returns the
rest:

```json
// query_range {"target": "entities/ahu-01/signals/supply-temp",
//              "from": "2026-11-02T08:00:00Z", "to": "2026-11-02T08:06:00Z", "limit": 3}
{
 "complete": false,
 "continuation": "7b2266223a383439343437303932353431383335383531312c226e223a22...",
 "count": 3,
 "rows": [
  {"quality": "GOOD", "sequence": 0, "signal": "entities/ahu-01/signals/supply-temp",
   "time": "2026-11-02T08:00:00Z", "value": 21.0},
  ...two more rows
 ]
}
```

**Was the second half warmer?** `compare_periods`, from the daemon's aggregates:

```json
// compare_periods {"target": "entities/ahu-01/signals/supply-temp",
//   "a": {"from": "2026-11-02T08:00:00Z", "to": "2026-11-02T08:03:00Z"},
//   "b": {"from": "2026-11-02T08:03:00Z", "to": "2026-11-02T08:06:00Z"}}
{
 "complete": true, "continuation": null, "count": 1,
 "rows": [{
   "signal": "entities/ahu-01/signals/supply-temp",
   "a": {"average": 21.633333333333336, "first": {"time": "2026-11-02T08:00:00Z", "value": 21.0},
         "last": {"time": "2026-11-02T08:02:00Z", "value": 22.4},
         "max": 22.4, "min": 21.0, "sample_count": 3, "sum": 64.9},
   "b": {"average": 22.099999999999998, "first": {"time": "2026-11-02T08:03:00Z", "value": 23.1},
         "last": {"time": "2026-11-02T08:05:00Z", "value": 21.2},
         "max": 23.1, "min": 21.2, "sample_count": 3, "sum": 66.3},
   "change": {"average": 0.46666666666666146, "max": 0.7000000000000028,
              "min": 0.1999999999999993, "sample_count": 0.0, "sum": 1.3999999999999915}
 }]
}
```

## Replay around an incident

A replay session plays history in order on a clock only the agent moves. `start` gives a
frozen session; `step` and `seek` return what they emitted; `stop` frees it.

```json
// replay {"action": "start", "target": "ahu-01",
//         "from": "2026-11-02T08:00:00Z", "to": "2026-11-02T08:06:00Z"}
{"clock": "2026-11-02T08:00:00Z", "expires": "2026-11-02T07:54:23.804592Z",
 "from": "2026-11-02T08:00:00Z", "samples_emitted": 0,
 "session": "replaySessions/replay-1", "state": "PAUSED", "to": "2026-11-02T08:06:00Z"}

// replay {"action": "step", "session": "replaySessions/replay-1", "events": 2}
{"clock": "2026-11-02T08:00:00Z", "count": 2, "ended": false, "omitted": 0,
 "events": [
  {"event": "sample", "signal": "entities/ahu-01/signals/fan-on",
   "time": "2026-11-02T08:00:00Z", "value": false, "quality": "GOOD", "sequence": 0},
  {"event": "sample", "signal": "entities/ahu-01/signals/supply-temp",
   "time": "2026-11-02T08:00:00Z", "value": 21.0, "quality": "GOOD", "sequence": 0}
 ],
 "samples_emitted": 2, "session": "replaySessions/replay-1", "state": "PAUSED", ...}

// replay {"action": "seek", "session": "replaySessions/replay-1", "time": "2026-11-02T08:04:00Z"}
{"clock": "2026-11-02T08:04:00Z", "count": 2, "ended": false, "omitted": 0,
 "events": [
  {"event": "state", "signal": "entities/ahu-01/signals/fan-on",
   "time": "2026-11-02T08:03:00Z", "value": true, ...},
  {"event": "state", "signal": "entities/ahu-01/signals/supply-temp",
   "time": "2026-11-02T08:03:00Z", "value": 23.1, ...}
 ], ...}

// replay {"action": "stop", "session": "replaySessions/replay-1"}
{"session": "replaySessions/replay-1", "state": "STOPPED"}
```

`expires` is the daemon's wall clock, which here was before the seeded data's times. A
seek reports each signal's state just before the target, so the agent knows where it
stands. A step too large to return is refused before the clock moves. Sessions left open
are stopped when `norn-mcp` exits.

## When something fails

A failure is a tool result the agent reads, with a stable `reason` and usually a `hint`.
With the daemon stopped:

```json
// list_entities {}
{
 "error": {
  "hint": "The Norn daemon is not answering at the endpoint this server was started with. Check it is running (`nornctl status`), then retry.",
  "message": "EntityService.ListEntities: transport I/O: No such file or directory (os error 2)",
  "metadata": {},
  "reason": "DAEMON_UNAVAILABLE",
  "retryable": true
 }
}
```

Once the daemon is back, the same call succeeds; `norn-mcp` need not restart. A malformed
`where` is the daemon's `ERROR_REASON_INVALID_ARGUMENT`, with `metadata.field` naming it
(`sample_filter`) and the hint naming the argument as the agent typed it, `where`. A
damaged chunk is `ERROR_REASON_DATA_CORRUPTED`, and the hint gives the instants to read
around. Every reason is in the [reference](../reference/mcp.md#errors).
