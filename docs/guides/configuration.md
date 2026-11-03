<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Configuration

How to configure the `norn` daemon. Its lifecycle is described in
[architecture.md](../reference/architecture.md#process-model-exists-stage-4).

```text
norn [--config PATH] [--listen ENDPOINT] [--data-dir PATH] [--log-level LEVEL] [--log-format FORMAT]
```

Each setting comes from the first of: a flag, the file named by `--config`, the
default. Without `--config` no file is read; there is no search path. Exit
status: 0 after a clean shutdown, 1 if the daemon failed, 2 for a command line
or configuration it refused.

## The file

TOML, at most 1 MiB. Every key is optional. An unknown key is refused rather
than ignored, so a typo cannot silently leave a limit at its default. This is
every key with its default:

```toml
listen = "unix:/path/to/norn.sock"  # default: the per-user endpoint, docs/reference/ipc.md
data_dir = "/path/to/norn"          # default: $XDG_DATA_HOME/norn, ~/.local/share/norn or %LOCALAPPDATA%\norn
drain_timeout_ms = 10000            # in-flight calls' time to finish on shutdown

[log]
level = "info"              # error, warn, info, debug, trace
format = "text"             # text, or json for one object per line
max_record_bytes = 4096     # at least 128; longer records are cut

[memory]                    # bytes; the subsystems must sum to at most the total
total_bytes = 536870912
ingest_bytes = 67108864
chunk_cache_bytes = 134217728
index_bytes = 67108864
query_bytes = 67108864
replay_bytes = 67108864
ipc_bytes = 33554432

[limits]
max_batch_samples = 10000
max_streams = 1024
max_entities = 100000
max_signals = 100000
max_query_page_samples = 10000
max_replay_sessions = 16
max_subscriptions = 256
max_clients = 64
max_pending_batches = 256
replay_session_idle_timeout_ms = 600000

[metadata]
max_entries = 64
max_key_bytes = 128
max_value_bytes = 1024

[storage]
max_segment_bytes = 268435456
max_segment_age_ms = 3600000
max_segment_open_time_ms = 3600000  # 1.0.0-rc.1: wall-clock time a segment may stay open
group_commit_interval_ms = 5   # the longest a DURABLE batch waits to share an fsync
max_chunk_age_ms = 60000       # the longest a sample waits to be encoded into a chunk
durable_acks = true            # false: no fsync, and DURABLE is refused, not weakened
```

The `[memory]`, `[limits]`, `[metadata]`, `[storage]` and `[maintenance]` defaults are
`norn_core::Config`'s, which documents each field and validates the result. They
were picked, not measured; stage 15 revisits them. The daemon applies the limits
of what exists (clients, IPC, ingest, entities, signals, streams, metadata) and
publishes them through `GetServer`; the rest wait for the subsystems they bound.

`max_entities` and `max_signals` bound `BatchCreateEntities` and `BatchCreateSignals`
too: a batch that would pass either fails whole with `RESOURCE_EXHAUSTED`. One batch
holds at most 1,000 requests. That bound is part of protocol 1.6, documented in the
schema, and not a key: a client must be able to rely on it whatever the daemon's file
says.

**Sealing** (1.0.0-rc.1). A segment seals when it reaches `max_segment_bytes`, when its
samples span `max_segment_age_ms` of *sample* time, when it has been open
`max_segment_open_time_ms` of the daemon's *wall-clock* time however little it holds,
and on a clean stop. The open-time limit is what lets a quiet or short-lived stream
reach sealed segments, which compaction, retention and the clean-start marker work on.
Zero is refused. Maintenance checks every quarter of the value, kept between 10 ms and
60 s, and does so even with `[maintenance] background = false`.

Half of `ipc_bytes` is set aside for HTTP/2 flow-control windows, an equal share
per client; the other half pays for messages being decoded ([architecture.md](../reference/architecture.md)).

`listen` takes `unix:<absolute path>`, `pipe:\\.\pipe\<name>` or
`tcp:<loopback address>:<port>`; [ipc.md](../reference/ipc.md) gives the syntax and the
security model. TCP has no OS identity check and is meant for tests.

## Maintenance

```toml
[maintenance]
background = true                      # false: only what a client asks for
compaction_interval_ms = 300000        # how often compaction looks for work
retention_interval_ms = 3600000        # how often recorded policies are applied
max_runs_per_pass = 8                  # the most merges per stream, per pass
compaction_window = "22:00-06:00"      # UTC; unset means any hour
```

The window limits when background compaction starts; `nornctl compact` ignores it.
These defaults were picked, not measured. The working memory comes from
`chunk_cache_bytes`.

## MCP

Off by default; gRPC on the local endpoint is always on and needs no setting.

```toml
[mcp]
enabled = true
transports = ["unix:/run/user/1000/norn/mcp.sock"]  # stdio, http:127.0.0.1:PORT, unix:PATH, pipe:NAME
hosts = []                                          # further Host names loopback HTTP answers
```

`--mcp TRANSPORT` on the command line adds a transport and enables MCP, so
`norn --mcp stdio` is all an agent's client needs to launch. Enabled with no
transport serves stdio. A non-loopback `http:` address is refused. The
[MCP reference](../reference/mcp.md#running-it) has every rule.

## Shared memory

New in 1.0.0-rc.1 (protocol 1.7), and off by default. When enabled, a client that asks for
it sends `AppendSamples` batches through a bounded iceoryx2 ring instead of the socket;
the socket still carries every response, and any call without a ring stays on the
socket.

```toml
[shared_memory]
enabled = false             # true: offer rings to clients that ask
service = "norn"            # iceoryx2 service name; the daemon appends its pid
max_rings = 2               # rings open at once; a call past it stays on the socket
slot_count = 4              # frames a ring holds before the writer waits
max_frame_bytes = 1048576   # the largest frame; a larger batch goes by the socket
```

While `enabled = false`, nothing else in the table is checked. When enabled, startup
refuses:

- `service` that is not 1 to 64 letters, digits, `-`, `_` or `.`
- `max_rings` outside 1 to 64 (each ring is a reader task)
- `slot_count` outside 1 to 1,024
- `max_frame_bytes` below 4 KiB or above the largest batch the socket takes
  (`max_batch_bytes`), so neither plane admits what the other refuses
- rings that do not fit: `max_rings × (slot_count + 2) × max_frame_bytes`, plus one
  frame for decoding, must fit the half of `ipc_bytes` not promised to HTTP/2 flow
  control

Rings are offered only on a Unix socket endpoint and never on Windows; configured but
unavailable, the daemon logs a warning and serves the socket alone. Enable it only
where every process of the daemon's user is trusted:
[shared memory](../reference/shared-memory.md#trust-boundary).

## Logging

Logs go to stderr; a supervisor owns files and rotation. Writes are synchronous,
so nothing queues: a stalled reader of stderr stalls logging, and the task that
logged, rather than growing memory. A record past `max_record_bytes` is cut and
ends ` [truncated]`, which in `json` format leaves that line unparsable.

## Turning fsync off

Tried for real, with this file passed as `--config`:

```toml
data_dir = "/tmp/norn-demo/x2"

[storage]
durable_acks = false
```

```console
$ nornctl status | grep capabilities
capabilities: CAPABILITY_APPEND_SAMPLES CAPABILITY_ACK_PERSISTED
```

The daemon stops offering `DURABLE` and refuses batches asking for it:
[ingesting data](ingesting-data.md#acknowledgement-levels).
