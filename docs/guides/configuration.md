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
group_commit_interval_ms = 5   # the longest a DURABLE batch waits to share an fsync
max_chunk_age_ms = 60000       # the longest a sample waits to be encoded into a chunk
durable_acks = true            # false: no fsync, and DURABLE is refused, not weakened
```

The `[memory]`, `[limits]`, `[metadata]` and `[storage]` defaults are
`norn_core::Config`'s, which documents each field and validates the result. They
were picked, not measured; stage 15 revisits them. The daemon applies the limits
of what exists (clients, IPC, ingest, entities, signals, streams, metadata) and
publishes them through `GetServer`; the rest wait for the subsystems they bound.

Half of `ipc_bytes` is set aside for HTTP/2 flow-control windows, an equal share
per client; the other half pays for messages being decoded ([architecture.md](../reference/architecture.md)).

`listen` takes `unix:<absolute path>`, `pipe:\\.\pipe\<name>` or
`tcp:<loopback address>:<port>`; [ipc.md](../reference/ipc.md) gives the syntax and the
security model. TCP has no OS identity check and is meant for tests.

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
