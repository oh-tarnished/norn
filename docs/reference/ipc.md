<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# IPC

How applications reach the Norn daemon (SPEC §5–7, §41). Two crates
implement it:

- **`norn-transport`** moves bytes. An `Endpoint` names a local address;
  `bind`/`connect` turn it into a `Listener` or a `BoxIo` stream; `Incoming`
  adds connection limits, backpressure, idle timeouts and abort. It imports
  no other Norn crate.
- **`norn-ipc`** runs gRPC on top: `serve`/`Server` host a tonic `Router` on
  an endpoint with a bounded graceful drain, and `connect` returns a tonic
  `Channel` through a custom connector. It also defines the `DataPlane` and
  `Attach` seams, and `AppendCall`, which uses a shared-memory ring when one is
  granted and the socket otherwise.
- **`norn-shared-memory`** implements those seams over iceoryx2, opt-in:
  [shared memory](shared-memory.md).

**The daemon's side** is now runtime-rs's `HybridServer` (`crates/server/src/daemon/serve.rs`),
on the same endpoints, with the same guarantees mapped onto its `GrpcLimits`: at most
`max_clients` connections with the next queued, a 10-minute idle close, the drain bound,
16 streams per connection and the flow-control windows below. Metered decoding, the
version gate, and the gate holding calls until the daemon runs are layers around every
service (`with_grpc_layer`), for socket and in-process callers alike. Its local listener
makes a Unix socket 0600 in a 0700 directory, replaces a stale one and refuses a live
one or a regular file, and a named pipe owner-only. `Daemon::bind` still claims the
endpoint first with `norn-ipc`'s checks, then releases it to the server, so a second
daemon fails before touching a database. `norn-ipc`'s `Server` remains for tests and
any other host; clients keep using `norn-ipc::connect`.

Clients never learn which transport they are on: the endpoint string picks
it, and everything above `norn-transport` sees only `AsyncRead + AsyncWrite`.

## Transports per OS

| OS | Default transport | Also available | Not available |
| --- | --- | --- | --- |
| Linux | Unix domain socket | loopback TCP (tests) | named pipes |
| macOS | Unix domain socket | loopback TCP (tests) | named pipes |
| Windows | named pipe | loopback TCP (tests) | Unix sockets* |

\* Windows 10 has AF_UNIX, but tokio does not support it there, and named
pipes are the native local IPC with a richer permission model. Asking for an
unsupported transport fails with `TransportError::Unsupported`; it never
falls back silently.

Platform code lives only in `crates/transport/src/platform/`: `mod.rs`
selects `unix.rs` or `windows.rs` with the crate's only `cfg`, and both
export the same five functions (`bind_unix`, `connect_unix`, `bind_pipe`,
`connect_pipe`, `default_endpoint`). Loopback TCP is portable and lives in
`tcp.rs`.

## Endpoint syntax

`<scheme>:<address>`; `Display` and `FromStr` round-trip.

| Form | Meaning |
| --- | --- |
| `unix:/run/user/1000/norn/norn.sock` | Unix socket; path must be absolute |
| `pipe:\\.\pipe\norn` | named pipe; only local `\\.\pipe\` names |
| `tcp:127.0.0.1:7171`, `tcp:[::1]:7171` | loopback TCP; port 0 binds an ephemeral port |

Parsing is pure and works for every scheme on every OS, so configuration can
be validated anywhere. Rejections are typed (`ParseEndpointError`): unknown
scheme, relative path, remote or empty pipe name, unparsable or
non-loopback TCP address.

**Defaults** (`Endpoint::platform_default()`):

- Unix: `$XDG_RUNTIME_DIR/norn/norn.sock`, else `$TMPDIR/norn/norn.sock`,
  else `/tmp/norn-$USER/norn.sock`.
- Windows: `\\.\pipe\norn-%USERNAME%` (or `\\.\pipe\norn` if the name has
  characters outside `[A-Za-z0-9_.-]`), so users on one machine never
  contend for a name.

## Security model (SPEC §41)

**Unix sockets.**

1. The socket's directory must be owner-only. A missing directory is
   created `0700` (parents included); an existing one with any group or
   other bit set is refused (`InsecureDirectory`). Ownership need not be
   checked separately: a `0700` directory we can create a socket in is ours
   (or we are root).
2. An occupied path is examined, never blindly unlinked: a socket that
   refuses connections is stale (a crashed daemon) and is replaced; a socket
   that accepts within one second is live and refused (`LiveSocket`);
   anything that is not a socket is refused and left alone (`NotASocket`).
3. After binding, the socket is `chmod 0600` regardless of umask.
4. On drop, the socket file is removed only if the path still holds the
   same device and inode we bound, so a successor's socket is never deleted.

**Named pipes.**

1. Every instance is created with a protected DACL, SDDL
   `D:P(A;;GA;;;<user SID>)`: generic-all for the user running the daemon
   and nothing else, not Administrators, not Everyone. (The default pipe
   DACL grants Everyone read and Administrators full control.)
2. `FILE_FLAG_FIRST_PIPE_INSTANCE` on the first instance: if another
   process already serves the name, bind fails. This is the analogue of
   refusing a live socket, and the defence against name squatting.
3. `PIPE_REJECT_REMOTE_CLIENTS`: SMB clients are refused.
4. Pipes are kernel objects; there is no stale file to clean up.

**TCP** is loopback-only, enforced at parse time and again at bind and
connect (`NotLoopback`). It has no OS identity check (any local user can
connect), which is why it is a test fallback and never a default.

**Unsafe.** None on Unix and none in `norn-ipc`. On Windows, building the
descriptor and finding the user's SID requires Win32 FFI (`windows-sys`):
std and tokio expose no safe API, and tokio accepts a descriptor only
through the `unsafe` `create_with_security_attributes_raw`. All of it is in
one module, `crates/transport/src/platform/windows/security.rs`, with a
`SAFETY` comment on every block, RAII guards for every allocation and
handle, and tests that run on the Windows CI runner.

## Limits and backpressure (SPEC §16, §41)

`Limits` is a builder over a meaningful `Default`, validated at bind:

| Setting | Default | Meaning |
| --- | --- | --- |
| `max_connections` | 64 (matches `norn_core` `max_clients`) | open connections at once; 1 ..= 65 536 |
| `overflow` | `Queue` | what happens to the next client at capacity |
| `idle_timeout` | 10 min | close a connection with no bytes in either direction |

- **`Overflow::Queue`**: stop accepting until a slot frees. Arrivals wait
  in the kernel's listen backlog (bounded by the OS, e.g. `SOMAXCONN`) or,
  on Windows, in the client's bounded busy-pipe retry (1 s). Bursts are
  delayed, not failed.
- **`Overflow::Refuse`**: accept and close immediately. The client sees EOF
  or a reset (at the gRPC level, `UNAVAILABLE`) and can retry or report.
  `Incoming::refused()` counts them.
- **Idle timeout** fails the pending read or write with `TimedOut`, so hyper
  tears the connection down through its normal error path. HTTP/2 PING
  frames count as traffic, so client keep-alives keep a quiet stream open.

Each admitted `Connection` owns a semaphore permit; dropping it frees the
slot. `ConnectionTracker` reports open connections and resolves when all are
gone. Per-connection memory beyond this is hyper/h2's, bounded by its
flow-control windows. The daemon sets `max_connections` from
`norn_core::Limits::max_clients` and each connection's window from
`MemoryBudgets::ipc_bytes` ([architecture.md, "Memory budgets"](architecture.md#memory-budgets), which also says
what that does not bound).

## Lifecycle and shutdown

- `Server::bind` claims the endpoint (a second daemon fails here, not later)
  and spawns nothing. A bound server can be inspected (e.g. its ephemeral
  port) or dropped, which releases the endpoint.
- `Server::serve(router, token)` runs until the `CancellationToken` is
  cancelled. Then the endpoint is released at once (no new clients), every
  connection gets HTTP/2 GOAWAY, and in-flight calls run to completion.
- **Bounded drain.** If connections remain after `drain_timeout` (default
  10 s), the server cancels its abort token: every remaining connection
  fails its next read or write with `ConnectionAborted`, and `serve` returns
  `ShutdownReport { aborted: n }`. If they still have not closed after a
  further second, it returns `IpcError::DrainStuck`.
- `serve` returns only after every connection task tonic spawned has ended.
  Handler futures for calls on aborted connections run as tonic's own tasks
  and finish on their own.
- `serve(endpoint, router, token)` is the one-call form with defaults.

## Client

`connect(&endpoint)` returns a `tonic::transport::Channel` connected eagerly
(a missing daemon fails here); `connect_lazy` defers to first use;
`connect_with(&endpoint, tonic::transport::Endpoint)` takes caller-set
timeouts, keep-alive and window sizes. All three dial through
`norn_transport::connect`, so the same code works for `unix:`, `pipe:` and
`tcp:`. A channel's clones share one HTTP/2 connection; tonic reconnects
through the same endpoint after a drop.

## Measured latency

Unary `grpc.health.v1.Health/Check` (tiny request and response), sequential
calls on one channel after 1 000 warm-up calls, release build. Reproduce
with `cargo run --release -p norn-ipc --example latency -- 20000`.

Apple M3 Max, macOS 27.0.1, rustc 1.99.1, tonic 0.14.9; two runs of 20 000
calls each (run 1 / run 2):

| Transport | p50 µs | p99 µs | mean µs | max µs |
| --- | --- | --- | --- | --- |
| Unix socket | 31.2 / 31.4 | 45.3 / 50.0 | 31.8 / 32.2 | 128 / 292 |
| loopback TCP | 46.0 / 45.8 | 63.8 / 67.2 | 47.2 / 46.9 | 217 / 210 |

Unix sockets are about a third faster at the median. Both are dominated by
HTTP/2 and gRPC framing and task wake-ups rather than the kernel copy, which
is why bulk data is meant for a separate data plane rather than a faster
socket. Linux and Windows figures are not yet measured.

## Data plane

SPEC §7 asks for shared memory where it pays, with regular IPC as the fallback.
Both exist, and the fallback is the default: bulk batches travel as gRPC messages
on the socket unless `[shared_memory] enabled = true` and the client opts in.
The full reference is [shared memory](shared-memory.md); in brief:

- `norn_ipc::DataPlane` is the seam (`kind()`, `max_frame()`, `send(frame)`), with
  a contract: bounded, ordered and lossless, crash-aware, `Send + Sync`.
  `ControlPlaneOnly` is the fallback, and `negotiate(ours, theirs)` picks the best
  plane both sides offer, defaulting to the control plane.
- `norn_ipc::Attach` turns a `SharedMemoryTicket` into a `DataPlane`;
  `norn_shared_memory::SharedMemory` implements it over iceoryx2. `norn-ipc` never
  imports `norn-shared-memory`, so a client without it is gRPC only.
- `norn_ipc::AppendCall` opens `AppendSamples`. Given
  `AppendOptions::shared_memory`, it negotiates protocol 1.7, reads
  `NegotiateProtocolResponse.shared_memory`, asks for a ring with the metadata
  `norn-data-plane: shared-memory`, and attaches to the ring the daemon names in
  `norn-shared-memory-service`. Anything missing along the way (an older daemon,
  shared memory off, no ring free, a failed attach) leaves the call on the socket.
- A frame is one encoded `AppendSamplesRequest`. Every response, including
  `DURABLE`, comes back on the gRPC call, from the same handler, after the same
  metering against `ipc_bytes`.
- Rings are offered only on a Unix socket endpoint, never on a named pipe or TCP,
  and never on Windows, where iceoryx2 cannot make a mapping owner-only.

The options evaluated before iceoryx2 was chosen (`memfd_create` with fd passing,
POSIX `shm_open`, a file in the socket directory, `CreateFileMapping`) all needed
`unsafe` mappings and a hand-written crash-safe ring; iceoryx2 provides both, and
Norn adds no `unsafe` of its own.

## Dependencies (SPEC §39)

| Crate | In | Why |
| --- | --- | --- |
| `tokio` (`net`, `time`, `sync`, `io-util`, `rt`) | transport, ipc | the runtime tonic needs; provides Unix sockets, named pipes and TCP itself |
| `tokio-util` | transport, ipc | `CancellationToken` for shutdown and abort; already in tonic's graph |
| `windows-sys` (Windows only) | transport | Win32 security-descriptor and token APIs; already in tokio's graph |
| `tonic` (`transport`, `router`) | ipc | gRPC (SPEC §8) |
| `hyper-util`, `tower`, `http`, `http-body`, `bytes`, `futures-core` | ipc | types tonic's custom-connector and custom-incoming hooks require; all already in tonic's graph |
| `tonic-health` | ipc (dev only), server, cli | the standard `grpc.health.v1` service and client |
| `norn-protocol`, `prost` | ipc | `AppendCall`: the `AppendSamples` and `NegotiateProtocol` clients, and encoding a batch into one frame |
| `iceoryx2` (`=0.10.0`) | shared-memory | the shared-memory ring; see [shared memory](shared-memory.md#dependencies-and-gates) |

No crate here links C; `xtask deps` checks it, with one audited exception for
iceoryx2's QNX-only build helpers ([shared memory](shared-memory.md#dependencies-and-gates)).

## Windows status

Named pipes run in CI on `windows-latest` (green at 7526100): the transport's
owner-only descriptor test opens a pipe as its own user, and `nornctl`'s daemon
test starts a real daemon on `pipe:\\.\pipe\norn-ctl-…` and drives `status`,
`version` and the resource commands over it. Which tests stay Unix-only, and why:
[operations](../guides/operations.md#platforms). Windows performance is not yet
measured.
