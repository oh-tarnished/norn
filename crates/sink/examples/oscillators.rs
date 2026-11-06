//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! A complete adapter for a toy protocol, the one docs/guides/device-adapter.md
//! walks through: a bank of oscillators that speak sine waves in fixed-size
//! frames. Everything protocol-specific is here, the codec and the mapping;
//! everything else is `norn_sink`'s.
//!
//! ```sh
//! cargo run -p norn-sink --example oscillators -- --endpoint unix:/tmp/n/n.sock --seconds 5
//! ```
//!
//! With no `--endpoint` it finds Norn as `norn_sink::endpoint` says, and may
//! therefore find one you are running: name yours if that matters.

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
