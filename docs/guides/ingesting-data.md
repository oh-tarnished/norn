<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Ingesting data

Ingest works today, through `nornctl append` for one sample and
`TelemetryService.AppendSamples` for real clients, and `nornctl import` for CSV,
Parquet, Arrow and MCAP files ([import and export](import-export.md)). Device
adapters maintained elsewhere use the same API ([adapters](../reference/adapters.md)),
most easily through `norn-sink` ([writing a device adapter](device-adapter.md)).

Create the [stream, entity and signal](getting-started.md#create-a-stream-an-entity-and-a-signal)
first. Output below is real.

## One sample

```console
$ nornctl append plant entities/dev-01/signals/temperature 21.75 --ack persisted --sequence 3
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
```

The first argument is the stream, the second the signal's full name. The value is
parsed as the type the signal declares. The first line is the admission, the
second the level you asked for; asking for `buffered` prints one line.

## Acknowledgement levels

Each level implies the ones below it. [protocol.md](../reference/protocol.md#acknowledgement-levels-spec-19)
has the exact definitions.

| Level       | Means                                    | Survives                                  |
| ----------- | ---------------------------------------- | ----------------------------------------- |
| `RECEIVED`  | read and validated, in process memory    | nothing                                   |
| `BUFFERED`  | in the stream's bounded write buffer     | nothing beyond the process                |
| `PERSISTED` | written to the journal, handed to the OS | the daemon exiting or crashing            |
| `DURABLE`   | fsynced (`F_FULLFSYNC` on macOS)         | power loss, if the device honours flushes |

Norn's tests kill the process; none cuts the power. The power-loss row is what
the design promises, not something that has been demonstrated.

With `durable_acks = false` the daemon refuses `DURABLE` rather than weakening it:

```console
$ nornctl append s entities/e/signals/v 1
nornctl: BACKPRESSURE_OUTCOME_REJECT: this daemon is configured without fsync 
$ nornctl append s entities/e/signals/v 1 --ack persisted
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_BUFFERED
BACKPRESSURE_OUTCOME_ACCEPT ACKNOWLEDGEMENT_LEVEL_PERSISTED
```

## Batches from a client

`AppendSamples` is a bidirectional stream, so a client keeps several batches in
flight and hears about backpressure without waiting on each write. A batch names
one stream and is admitted or rejected whole.

[`crates/cli/examples/plain_client.rs`](../../crates/cli/examples/plain_client.rs)
is a complete client with nothing of the daemon in it. Its core:

```rust
tx.send(AppendSamplesRequest {
    stream: "streams/plant".into(),
    batch_id: 1,
    samples: vec![Sample {
        signal: "entities/dev-01/signals/temperature".into(),
        observe_time: Some(prost_types::Timestamp { seconds, nanos: 0 }),
        value: Some(Value { kind: Some(Kind::DoubleValue(21.5)) }),
        quality: Some(Quality { severity: QualitySeverity::Good as i32, substatus: 0 }),
        sequence: 1,
    }],
    ack_level: AcknowledgementLevel::Durable as i32,
})
```

```console
$ cargo run -q -p norn-cli --example plain_client -- unix:/tmp/norn-demo/p/n.sock
batch 1 reached ACKNOWLEDGEMENT_LEVEL_BUFFERED
batch 1 reached ACKNOWLEDGEMENT_LEVEL_DURABLE
```

## Backpressure

Every batch hears one outcome. Nothing is dropped silently.

| Outcome    | Meaning                                                            |
| ---------- | ------------------------------------------------------------------ |
| `ACCEPT`   | admitted, with capacity to spare                                   |
| `BUFFER`   | admitted, but acknowledgements beyond `BUFFERED` will lag          |
| `THROTTLE` | admitted; wait `retry_delay` before the next batch for this stream |
| `REJECT`   | refused whole; the reason says whether a retry can work            |

In the sessions above only `ACCEPT` and `REJECT` appeared; `BUFFER` and
`THROTTLE` are exercised by `crates/runtime/tests/backpressure.rs`. Full table:
[protocol.md](../reference/protocol.md#backpressure-outcomes-spec-18).

## Refused samples

A sample for a signal in another stream, or a stream that does not exist:

```console
$ nornctl append other entities/dev-01/signals/temperature 1
nornctl: BACKPRESSURE_OUTCOME_REJECT: 1 samples are invalid entities/dev-01/signals/temperature records into another stream
$ nornctl append missing entities/dev-01/signals/temperature 1
nornctl: BACKPRESSURE_OUTCOME_REJECT: streams/missing does not exist 
```

## Duplicates

A resend of a sample equal in signal, time and sequence is counted and not stored
again, within the limits of a bounded window.
[storage.md](../reference/storage.md#duplicates-exactly-what-is-guaranteed) says
exactly what is guaranteed. `nornctl append` stamps the time itself, so running
the same command twice stores two samples.

## After ingest

`nornctl streams` shows counts, and a sample acknowledged at `PERSISTED` or better is
already queryable: see [querying](querying.md).
