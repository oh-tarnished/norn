# AI agents

The `norn` daemon serves an AI agent Norn's history through the Model Context Protocol
when asked: what exists, what happened over an interval, what everything held at an
instant, when something changed, statistics and comparisons, and a step-by-step replay.
Every tool is a read-only RPC of the public schema, generated from its annotations, and
the reasoning is the agent's. The design, every tool and every bound are in the
[MCP reference](../reference/mcp.md).

## Build

```sh
cargo build -p norn-server -p norn-cli
```

Building `norn` needs a C compiler, for `ring`, which runtime-rs's server brings
([architecture](../reference/architecture.md#no-c)).

## Serve MCP

MCP is off by default. `--mcp` turns it on, once per transport:

```console
$ norn --help
usage: norn [OPTIONS]

Runs the Norn daemon in the foreground until SIGINT or SIGTERM (Ctrl-C on
Windows), then drains in-flight calls and exits.

options:
  --config PATH        read settings from a TOML file (docs/guides/configuration.md)
  --listen ENDPOINT    serve on ENDPOINT, such as unix:/run/norn/norn.sock,
                       pipe:\\.\pipe\norn or tcp:127.0.0.1:7171
                       (default: the platform's per-user endpoint)
  --data-dir PATH      keep the database in PATH (default: per-user,
                       docs/guides/configuration.md)
  --mcp TRANSPORT      also serve MCP: stdio, http:127.0.0.1:PORT, unix:PATH or
                       pipe:NAME. Repeatable (docs/reference/mcp.md)
  --log-level LEVEL    error, warn, info (default), debug or trace
  --log-format FORMAT  text (default) or json
  -h, --help           print this text
  -V, --version        print the daemon and protocol versions

Flags override the file; the file overrides the built-in defaults.
```

Or in the configuration file, `[mcp] enabled = true` with `transports`
([MCP reference](../reference/mcp.md#running-it)).

Over stdio the agent's client launches the daemon itself, and the daemon stops when the
client leaves. For a daemon that runs on its own, serve MCP on a Unix socket or a named
pipe beside gRPC, or on loopback HTTP. Anything but loopback is refused:

```console
$ norn --mcp http:0.0.0.0:8090
norn: invalid --mcp: http:0.0.0.0:8090: only a loopback address may serve MCP
```

## Configure a client

A client that launches servers over stdio, such as Claude Desktop:

```json
{
  "mcpServers": {
    "norn": {
      "command": "/path/to/norn/target/debug/norn",
      "args": ["--mcp", "stdio"]
    }
  }
}
```

With Claude Code, `claude mcp add norn -- /path/to/norn --mcp stdio` does the same. A
client that speaks streamable HTTP connects to `http://127.0.0.1:8090/mcp` of a daemon
started with `--mcp http:127.0.0.1:8090`.

## Using the tools

The tools are the schema's read methods, named `<service>-<method>_v1`, and take the
method's request as JSON: `parent` is `entities/{entity}` or `entities/-`, `filter`
selects signals (`id = "supply-temp"`), and times are RFC 3339. A paginated read returns
`nextPageToken`; pass it back as `page_token` to read on.

- To find when a supply temperature went above 80: `query_service-find_events_v1` with
  `kind: EVENT_KIND_MATCH` and `sample_filter: "value > 80"`.
- To see what changed: `kind: EVENT_KIND_VALUE_CHANGE`. A signal's first sample in the
  interval is the baseline; widen the interval to see a change at its start.
- To compare this week with last: `query_service-compare_periods_v1` with
  `baseline_interval` and `comparison_interval`.
- To replay around an incident: `replay_service-create_replay_session_v1` with
  `speed: 0`, then `replay_service-drive_replay_session_v1` with `seek_time`,
  `event_count` or `duration`, then `replay_service-delete_replay_session_v1`.

## What the tools cannot do

The surface is read-only by construction: the daemon lists and dispatches only the
methods the schema hints read-only, 17 of them under protocol 1.6. Writes keep their
RPCs and have no tool. That includes `EntityService.BatchCreateEntities` and
`SignalService.BatchCreateSignals`, added in 1.6 for provisioning: the schema
annotates them `read_only: false`, so an agent can see them described in the schema
but never call them. Provisioning is a client's job, over gRPC
([protocol](../reference/protocol.md#writing-a-client)).

## When something fails

A failed call is a result the agent reads, `{"error": {"reason", "message", "retryable",
"metadata"}}`; branch on `reason`. A step too big to return is refused before the clock
moves (`ERROR_REASON_LIMIT_EXCEEDED`); step fewer events. A session the agent did not
create is `MCP_UNKNOWN_SESSION`. The [reference](../reference/mcp.md#errors) lists every
reason.
