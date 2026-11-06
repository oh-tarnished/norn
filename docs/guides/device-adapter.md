<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Writing a device adapter

A device adapter brings live readings from a device protocol into Norn. It is a
separate program, a client of the daemon's public API and nothing else
([SPEC section 47](../project/spec.md#47-integration-rule)), and it lives in its own
repository beside the protocol library it uses.

Most of what makes such a client correct has nothing to do with the protocol:
surviving the daemon restarting, honouring backpressure, resending what was not
acknowledged, not dropping a sample without counting it, creating tens of thousands
of signals without holding up the readings. The `norn-sink` crate
([`crates/sink`](../../crates/sink)) is all of that. You write two things: a codec
for your protocol, and a mapping from what it speaks of onto entities and signals.

This guide builds a complete adapter for a toy protocol.
[`crates/sink/examples/oscillators.rs`](../../crates/sink/examples/oscillators.rs) is
the same program, and it compiles and runs.

## What you write, and what you do not

| Yours | `norn-sink`'s |
| --- | --- |
| Talking to the device: sockets, polling, subscriptions | Finding the daemon, and connecting again when it goes away |
| Decoding the protocol's frames | The bounded byte buffer, with a share per signal |
| Which entities and signals exist: a [`Catalog`](#step-2-map-onto-entities-and-signals) | Batch creation, cut to the message limit, safe at every start |
| One [`Reading`](#step-2-map-onto-entities-and-signals) per observation | `AppendSamples`, backpressure, acknowledgement levels, resend |
| What to do about a [`Notice`](#step-4-listen) | The daemon's drain, health, counts |

## Step 1: Depend on the crate

```toml
[dependencies]
norn-sink = { git = "https://github.com/shokki-engineering/norn", tag = "..." }
tokio = { version = "1", features = ["macros", "rt", "time"] }
```

The crate depends on `norn-ipc`, `norn-protocol`, `norn-transport` and `tokio`.
Everything else in Norn stays out of your program, and the crate is never imported
by the engine or the daemon (`xtask arch` enforces both).

## Step 2: Map onto entities and signals

An **entity** is a thing that exists. A **signal** is one measurable property of it,
of one value type for life. A **reading** is one value of a signal at the source's
time.

- IDs are 1 to 63 of `[a-z0-9-]`, starting with a letter.
  `norn_sink::ids::slug` folds free text into that alphabet. An ID Norn would refuse
  is a typed error at the call that made it.
- Your protocol's own vocabulary (addresses, object kinds, vendor names) goes in
  annotations, which Norn indexes for filtering and never interprets.
- Pick a value type you can keep. Declaring a signal again with another type is
  `SinkError::TypeChanged`, because stored chunks are encoded for it.
- Give each reading the source's own time and, if the source numbers its messages,
  its number as `Reading::sequence`. Norn stores a sample once by its signal, time and
  sequence, so that is what makes a resend harmless. A reading with none is given the
  sink's next count.
- Bad quality is still history: say so with `Reading::quality`, and never alter or
  skip the value.

## Step 3: Run it

```rust
let (at, rule) = norn_sink::endpoint::locate(flag_or_config)?;
let sink = Sink::new(at, "my-stream", Options::default())?;
let running = tokio::spawn({
    let sink = sink.clone();
    async move { sink.run().await }
});
sink.declare(&catalog)?;   // never waits on the daemon
sink.push(readings)?;      // never waits on the daemon
sink.close().await;        // drains, then the run returns
let report = running.await??;
```

- `Sink::new` spawns nothing and touches nothing. The daemon need not be up.
- `run` is the one loop that talks to the daemon. Your task owns it, and it starts no
  task of its own. A daemon that goes away is not an error: it waits, connects again,
  and resends.
- `declare` and `push` return at once from any task, so a slow or absent Norn slows
  nothing in your program. `declare` as you discover: it is incremental, and what is
  declared again unchanged costs nothing.
- `close` is idempotent. It gives what is held `Options::drain` (10 s by default) to
  be acknowledged. What is left is counted in `Report::never_sent`.

### Which Norn

`endpoint::locate` takes the first of:

1. the endpoint you name, from a flag or your configuration;
2. `NORN_ENDPOINT`;
3. the [Homebrew service's](homebrew.md) socket, under `HOMEBREW_PREFIX`,
   `/opt/homebrew`, `/usr/local` or `/home/linuxbrew/.linuxbrew`;
4. Norn's platform default, which a `norn` started by hand with no `--listen` serves.

A mistyped endpoint is an error naming the rule that gave it, never a reason to try
the next one. Log what you got, as the example does. Point tests at a daemon of their
own, because rule 3 finds a Norn you may be running.

## Step 4: Listen

The daemon being away, slow or draining is expected, so it is never returned as an
error. `SinkError` means the adapter made a mistake.

- `Options::on_notice` is called with each `Notice` to log or count. It must not block.
- `Sink::reach` is a `watch` of whether Norn answers.
- `Sink::health` is a snapshot for a status page: delivered, held, dropped, degraded,
  and signals still unmade.
- `Sink::counts` accounts for every reading pushed. Each ends in exactly one of
  `acknowledged`, `overflowed`, `gated`, `invalid` or `never_sent`, or is still held.

## What happens to a reading

**Held until acknowledged.** A reading stays in the sink until Norn has acknowledged
it at `Options::ack`: `PERSISTED` by default (SPEC section 19). If a call dies, what
was unanswered goes again on the next one, keeping its time and sequence, so Norn
counts it as a duplicate and stores it once.

**Waiting only for its own signal.** A reading of a signal not yet created waits, but
only for that signal. A cold start can declare 20,000 signals while readings of those
already made keep flowing.

**Bounded.** `Options::buffer_bytes` (64 MiB by default) bounds what is held, and each
signal may hold a sixty-fourth of it, never under 64 KiB. A signal past its share has
its new readings given up (`gated`), so one chatty signal cannot starve the rest. Past
the whole bound the oldest go, whoever's (`overflowed`). The bound is on memory, so it
cannot be a wait: `push` never blocks. Every reading given up is counted and told as
`Notice::Dropped`, and `Health::degraded` stays true for `Options::degraded_for`.

**Backpressure.** Norn's four answers ([ingesting data](ingesting-data.md)) are handled
for you:

| Answer | What the sink does |
| --- | --- |
| `ACCEPT`, `BUFFER` | Goes on |
| `THROTTLE` | The batch is in; the next waits `retry_delay` |
| `REJECT`, transient | Holds the batch, and resends after `retry_delay` or a backoff |
| `REJECT`, `INVALID_SAMPLES` | Gives up and counts the listed samples, and sends the rest |
| `REJECT`, `BATCH_TOO_LARGE` | Sends smaller batches from then on |
| `REJECT`, anything else | Tells you, holds, and backs off: the buffer's bound says how long you have |

**A drain is not a failure.** A daemon stopping answers what it admitted and ends the
call with `UNAVAILABLE` and `ERROR_REASON_SHUTTING_DOWN`. The sink reconnects and
resends, with a `Notice::Draining`. `Reach` does not turn down unless the new call
cannot be had.

**Provisioning.** Entities go first, then signals in `BatchCreate` batches, one on each
connection at once, cut to three quarters of the daemon's unary message limit:
the one it publishes, else `Options::unary_limit`, 256 KiB by default. A batch refused
`RESOURCE_EXHAUSTED` is sent again in halves. A daemon with no batch create gets one
call each. A restart re-declares everything safely, and only what has changed since is
updated. A signal's value type is never updated.

## Shared memory

Build with the `shared-memory` feature and set `Options::shared_memory(true)` to ask
for a [ring](../reference/shared-memory.md) on each call. Every answer still arrives
over gRPC, so nothing else changes. A daemon that grants none leaves the call on the
socket, and `Notice::Plane` says which you got. It is off by default on both sides.
Read what a ring trusts in that reference first.

## A complete adapter

The whole of the example. It has three parts: the protocol, the mapping, and a `main`
that is the only part touching the kit. A real adapter replaces `sweep` with its
transport.

```rust
use norn_sink::{
    Catalog, Entity, Kind, Options, QualitySeverity, QualitySubstatus, Reading, Signal, Sink,
    ValueType, endpoint,
};
use std::f64::consts::TAU;
use std::time::{Duration, SystemTime};

/// Oscillators in the bank, and channels on each.
const BANK: u16 = 4;
const CHANNELS: u8 = 3;
/// Frames each channel sends a second.
const RATE: u64 = 20;

// ---- The protocol: its wire format and a stand-in for its transport. ------

/// One frame: which channel, its count, and a value in thousandths.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Frame {
    oscillator: u16,
    channel: u8,
    count: u32,
    millis: i32,
    /// The source's own opinion of the value.
    clipped: bool,
}

const FRAME_BYTES: usize = 12;

fn encode(f: Frame) -> [u8; FRAME_BYTES] {
    let mut out = [0; FRAME_BYTES];
    out[0..2].copy_from_slice(&f.oscillator.to_be_bytes());
    out[2] = f.channel;
    out[3] = u8::from(f.clipped);
    out[4..8].copy_from_slice(&f.count.to_be_bytes());
    out[8..12].copy_from_slice(&f.millis.to_be_bytes());
    out
}

fn decode(b: &[u8; FRAME_BYTES]) -> Frame {
    Frame {
        oscillator: u16::from_be_bytes([b[0], b[1]]),
        channel: b[2],
        clipped: b[3] != 0,
        count: u32::from_be_bytes([b[4], b[5], b[6], b[7]]),
        millis: i32::from_be_bytes([b[8], b[9], b[10], b[11]]),
    }
}

/// What the wire would carry at `count`: every channel of every oscillator.
fn sweep(count: u32) -> Vec<[u8; FRAME_BYTES]> {
    let mut frames = Vec::new();
    for oscillator in 0..BANK {
        for channel in 0..CHANNELS {
            let hertz = 0.1 * f64::from(u16::from(channel) + 1);
            let phase = f64::from(oscillator) / f64::from(BANK);
            let t = f64::from(count) / RATE as f64;
            let value = 1.2 * (TAU * (hertz * t + phase)).sin();
            frames.push(encode(Frame {
                oscillator,
                channel,
                count,
                millis: (value.clamp(-1.0, 1.0) * 1000.0) as i32,
                clipped: value.abs() > 1.0,
            }));
        }
    }
    frames
}

// ---- The mapping: this protocol's things as entities and signals. ---------

fn entity_id(oscillator: u16) -> String {
    format!("oscillator-{oscillator}")
}

fn signal_id(channel: u8) -> String {
    format!("channel-{channel}")
}

/// What readings need to exist. The protocol's own vocabulary goes in
/// annotations, which Norn indexes and never reads.
fn catalog() -> Catalog {
    let mut catalog = Catalog::new();
    for oscillator in 0..BANK {
        let entity = entity_id(oscillator);
        catalog.push_entity(
            Entity::new(&entity)
                .display_name(format!("Oscillator {oscillator}"))
                .annotation("toy.bank.slot", oscillator.to_string()),
        );
        for channel in 0..CHANNELS {
            catalog.push_signal(
                Signal::new(&entity, signal_id(channel), ValueType::Double)
                    .display_name(format!("Oscillator {oscillator} channel {channel}"))
                    .unit("volt")
                    .annotation("toy.channel", channel.to_string()),
            );
        }
    }
    catalog
}

/// One frame as one reading: the source's count as its sequence, so a frame sent
/// twice is stored once, and its doubt as quality, which never alters a value.
fn reading(frame: Frame, at: SystemTime) -> Reading {
    let value = Kind::DoubleValue(f64::from(frame.millis) / 1000.0);
    let entity = entity_id(frame.oscillator);
    let reading = Reading::new(&entity, &signal_id(frame.channel), at, value);
    let reading = reading.sequence(i64::from(frame.count));
    if frame.clipped {
        reading.quality(QualitySeverity::Uncertain, QualitySubstatus::Unspecified)
    } else {
        reading
    }
}

// ---- The adapter: the only part that touches the kit. ---------------------

fn flag(name: &str) -> Option<String> {
    let mut args = std::env::args().skip_while(|a| a != name);
    args.next();
    args.next()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (at, rule) = endpoint::locate(flag("--endpoint").as_deref())?;
    let seconds: u64 = flag("--seconds").map_or(Ok(5), |s| s.parse())?;
    eprintln!("Norn at {at} (from {rule})");

    let options = Options::default()
        .user_agent("oscillators/1")
        .on_notice(|notice| eprintln!("{notice}"));
    let sink = Sink::new(at, "oscillators", options)?;
    let running = tokio::spawn({
        let sink = sink.clone();
        async move { sink.run().await }
    });

    sink.declare(&catalog())?;

    let mut tick = tokio::time::interval(Duration::from_millis(1000 / RATE));
    for count in 0..u32::try_from(seconds * RATE)? {
        tick.tick().await;
        let now = SystemTime::now();
        let frames = sweep(count);
        sink.push(frames.iter().map(|bytes| reading(decode(bytes), now)))?;
    }

    sink.close().await;
    let report = running.await??;
    println!(
        "{} readings acknowledged, {} dropped, {} never sent",
        report.counts.acknowledged,
        report.counts.dropped(),
        report.never_sent
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_survives_the_wire() {
        for bytes in sweep(7) {
            assert_eq!(encode(decode(&bytes)), bytes);
        }
    }
}
```

## Run it

Against a daemon of its own, so it cannot touch one you are running:

```sh
mkdir -m 700 -p /tmp/norn-try/ipc
cargo run -p norn-server --bin norn -- \
  --listen unix:/tmp/norn-try/ipc/n.sock --data-dir /tmp/norn-try/data &

cargo run -p norn-sink --example oscillators -- \
  --endpoint unix:/tmp/norn-try/ipc/n.sock --seconds 2
```

Which prints:

```text
Norn at unix:/tmp/norn-try/ipc/n.sock (from the adapter's configuration)
Norn reachable
12 signals made; no reading waits on one
480 readings acknowledged, 0 dropped, 0 never sent
```

## Testing yours

`crates/sink/tests/delivery` runs the sink against a fake daemon over a real socket,
and `crates/sink/tests/real_daemon` against a real one in a temporary directory. Your
adapter needs neither of those again: test your codec and your mapping, which are pure
functions, and one run against a real daemon.
