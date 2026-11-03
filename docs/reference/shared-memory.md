<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Shared memory

An opt-in data plane for bulk sensor data: `AppendSamples` batches travel through
a bounded [iceoryx2](https://github.com/eclipse-iceoryx/iceoryx2) ring in shared
memory, while the Unix socket stays the control plane and carries every
response. It is **off by default**. Whenever a ring is not available, a client
falls back to the socket on the same call, with nothing for the caller to do, and
the answers are the same either way.

Crates: `norn-shared-memory` implements the `DataPlane` and `Attach` seams that
`norn-ipc` declares ([IPC](ipc.md#data-plane)); the daemon grants rings
(`crates/server/src/telemetry/plane.rs`); `norn_ipc::AppendCall` is the client.

## Turning it on

```toml
[shared_memory]
enabled = true          # default false
service = "norn"        # iceoryx2 service name; the daemon appends its pid
max_rings = 2           # rings open at once; a call past it stays on the socket
slot_count = 4          # frames a ring holds before the writer waits
max_frame_bytes = 1048576  # the largest frame; a larger batch goes by the socket
```

While off, nothing in the table is checked. When on, startup refuses:

| Setting | Bound |
| --- | --- |
| `service` | 1 to 64 letters, digits, `-`, `_` or `.` |
| `max_rings` | 1 to 64 (each ring is a reader task) |
| `slot_count` | 1 to 1,024 |
| `max_frame_bytes` | 4 KiB up to the largest batch the socket takes (`Limits.max_batch_bytes`), so neither plane admits what the other refuses |
| all rings | `max_rings × (slot_count + 2) × max_frame_bytes`, plus one frame for decoding, must fit the half of `ipc_bytes` not promised to HTTP/2 flow control |

The daemon offers rings only on a Unix socket endpoint, because only its `0700`
directory proves the client is the daemon's user, and never on Windows (below).
Configured but unavailable, it logs a warning and serves the socket alone.

## Protocol 1.7

- `CAPABILITY_SHARED_MEMORY` is in `Server.capabilities` and in
  `NegotiateProtocolResponse.capabilities` when rings are offered.
- `NegotiateProtocolResponse.shared_memory` (`SharedMemoryPlane`) says how to
  attach. It is set only when the capability is offered **and** the agreed minor
  version is 7 or later, so an older client never sees it:

| Field | Meaning |
| --- | --- |
| `service` | iceoryx2 service name every ring's name starts with: `{service}/{ring}` |
| `root_directory` | the owner-only directory iceoryx2 keeps its files in |
| `name_prefix` | the prefix of every name iceoryx2 creates for this daemon |
| `max_frame_bytes` | the largest frame a ring takes |
| `slot_count` | frames a ring holds |
| `max_rings` | rings open at once |
| `segment_bytes` | shared memory one ring maps, charged to `ipc_bytes` while open |

- Per call, in gRPC metadata: the client sends `norn-data-plane: shared-memory` on
  `AppendSamples`; a daemon that grants a ring answers with the response metadata
  `norn-shared-memory-service`, the ring's whole service name. Metadata, because a
  bidirectional stream has no message the daemon could answer before the first
  batch, and the ring must exist before that batch is written into it. The client
  refuses a name outside the negotiated `service`.

## How a call runs

1. The client negotiates, asks for a ring and, if granted, attaches as the ring's
   only publisher. The daemon created the service (one publisher, one subscriber,
   two nodes, no history, no overflow), so the client cannot choose a limit.
2. Each batch is the same `AppendSamplesRequest` protobuf, written as one frame.
   A full ring makes the writer wait: backpressure, never loss.
3. The daemon's reader, a blocking task on the same tracked set as the call,
   treats each frame as hostile: it checks the length iceoryx2 claims against
   `max_frame_bytes` before reading, reserves the worst-case decoding cost from
   `ipc_bytes` (waiting up to 30 s, as a socket message does), copies the frame
   out once (the writer can rewrite shared memory while it is read), counts its
   samples before decoding, and decodes only the copy. It is the socket path's own
   arithmetic (`crates/server/src/metered/cost.rs`).
4. From there the batch goes through the same handler as a socket batch:
   validation, `max_batch_samples`, `max_pending_batches`, admission outcomes,
   ordering, and `RECEIVED`, `BUFFERED`, `PERSISTED` and `DURABLE`
   acknowledgements, all on the gRPC response stream ([protocol](protocol.md)).
5. Closing the request side ends the call: the daemon answers what is in the ring
   and removes the service. The client's response half keeps the publisher alive
   until the replies end or are dropped, so closing the sender cannot remove
   frames before the daemon reads them. Shutdown waits for the reader with the calls.

**Order.** A batch larger than a frame goes by the socket. The daemon holds a
socket batch until the ring is empty, so it lands after everything published
before it; the client then stays on the socket for the rest of the call, since the
daemon cannot know a socket batch is coming and a later frame could overtake it.

**Writer crash.** iceoryx2 publishes a frame only when it is sent, after it is
written, so a writer killed mid-frame publishes nothing. Its broken call stops
the reader, which answers every whole frame already published, then frees the
ring for the next call. Tested by killing a real child process mid-frame and in
a flood: every stored batch is whole and every sample is stored once
(`crates/server/tests/shared_memory_crash.rs`).

**Lost sends are errors.** iceoryx2 reports a send that reached no subscriber as
success. The producer reads that as "full" while the daemon is attached and as
`BrokenPipe` when not, and a writer waiting on a ring whose daemon died is
released when the gRPC call breaks. A send that fails leaves the batch's fate
unknown: resend it on a new call; sequence numbers make the repeat a duplicate.

## Files and permissions

The daemon points iceoryx2 at `<socket directory>/iox2/`, created `0700` inside
the `0700` socket directory, with a per-daemon prefix `norn<hash>_`, instead of
iceoryx2's world-listable `/tmp/iceoryx2`. iceoryx2 creates every shared-memory
object `0600` (its `dev_permissions` feature stays off). Outside that directory:
on macOS, a state file per object in `/tmp` and the POSIX object it names; on
Linux, objects in `/dev/shm`. Names carry hashes, never data.

## Trust boundary

Only processes of the daemon's user can open a ring. iceoryx2 trusts what it finds
in its shared bookkeeping, so a misbehaving process **of the same user** can crash
the daemon by writing there. Such a process could equally signal the daemon or
edit its files, so shared memory does not widen who can harm Norn, only how; but
it turns a bug in any program of that user into a possible daemon crash. That is
why it is off by default: enable it where every process of that user is trusted.
`norn-shared-memory` is `#![forbid(unsafe_code)]`; iceoryx2's own `unsafe` is not
audited by this project.

## Windows

iceoryx2 0.10 creates file mappings with no security attributes, so a mapping
gets the token's default descriptor, not an owner-only one, and offers no way to
pass one. The daemon therefore never offers rings on Windows; clients stay on the
named pipe.

## Dependencies and gates

iceoryx2 is pinned at `=0.10.0` (pre-1.0; client and daemon must agree on the
segment layout). Its `enum-iterator` dependency is 0BSD, allowed in `deny.toml`
by the owner's decision of 2026-11-03. Two of its platform crates name `cc` and
`bindgen` as build dependencies that run only on a QNX build host;
`xtask/src/iceoryx2.rs` excuses exactly those helpers, of exactly those crates,
beneath `norn-shared-memory`, `norn-server` and `norn-cli` only, and fails any
other crate that reaches iceoryx2 (`xtask/tests/iceoryx2_gates.rs`).

## Measuring it

```sh
cargo bench -p norn-server --bench shared_memory
nornctl import samples.parquet --ack buffered --timing           # the socket
nornctl import samples.parquet --ack buffered --shared-memory    # a ring
```

An import is safe to repeat, so a second import of the same file sends only what
is missing: compare the two on fresh daemons. `append --shared-memory` prints
`plane SHARED_MEMORY` when its sample went through a ring.

The benchmark drives one daemon at 1,000, 10,000 and 100,000 samples a second,
each load once over the socket and once over a ring, through the same client
code, and prints achieved rate, acknowledgement latency and peak memory. No
figures are recorded here yet.
