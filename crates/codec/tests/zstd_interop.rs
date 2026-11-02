//! Cross-checks the pure-Rust zstd output against the reference C `zstd`
//! command-line tool (SPEC §14: frames must be standard). The CLI is used
//! here only, never as a dependency.
//!
//! When `zstd` is not on `PATH` the tests log and pass, unless
//! `NORN_REQUIRE_ZSTD_CLI=1` is set, in which case they fail: CI that means
//! to prove interop sets it, so a missing tool cannot pass silently.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    missing_docs
)]

use std::io::Write;
use std::process::{Command, Stdio};

use norn_codec::compress::{self, Compression, Stored, ZstdLevel};
use norn_codec::{float64, timestamp};

fn cli(args: &[&str], input: &[u8]) -> Option<Vec<u8>> {
    let child = Command::new("zstd")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn();
    let Ok(mut child) = child else {
        assert!(
            std::env::var("NORN_REQUIRE_ZSTD_CLI").as_deref() != Ok("1"),
            "zstd CLI required but missing"
        );
        eprintln!("zstd CLI not found; interop check skipped");
        return None;
    };
    child.stdin.take().unwrap().write_all(input).unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success(), "zstd {args:?} failed");
    Some(out.stdout)
}

/// A realistic encoded block: a day of 1 Hz drift readings plus timestamps.
fn block() -> Vec<u8> {
    let mut b = Vec::new();
    let ts: Vec<i64> = (0..86_400)
        .map(|i| i * 1_000_000_000 + (i % 13) * 7_000)
        .collect();
    timestamp::encode(&ts, &mut b).unwrap();
    let v: Vec<f64> = (0..86_400)
        .map(|i| ((i as f64 / 3000.0).sin() * 30.0).round() / 10.0 + 21.0)
        .collect();
    float64::encode(&v, &mut b).unwrap();
    b
}

#[test]
fn every_level_produces_frames_the_reference_decoder_reads() {
    let block = block();
    for level in 3..=6 {
        let mut sealed = Vec::new();
        let how = Compression::Zstd(ZstdLevel::new(level).unwrap());
        assert_eq!(
            compress::seal(&block, how, &mut sealed).unwrap(),
            Stored::Zstd(ZstdLevel::new(level).unwrap())
        );
        // Envelope: kind, level, varint length; the frame follows.
        let frame_at = 2 + sealed[2..].iter().position(|b| b & 0x80 == 0).unwrap() + 1;
        let Some(plain) = cli(&["-d", "-c", "-q"], &sealed[frame_at..]) else {
            return;
        };
        assert_eq!(plain, block, "level {level}");
    }
}

#[test]
fn reference_encoder_frames_decode_through_the_envelope() {
    let block = block();
    for level in ["-3", "-6", "-19"] {
        let Some(frame) = cli(&[level, "-c", "-q"], &block) else {
            return;
        };
        let mut envelope = vec![1u8, 3];
        let mut n = block.len() as u64;
        while n >= 0x80 {
            envelope.push(n as u8 | 0x80);
            n >>= 7;
        }
        envelope.push(n as u8);
        envelope.extend_from_slice(&frame);
        let mut out = Vec::new();
        compress::open(&envelope, &mut out).unwrap();
        assert_eq!(out, block, "reference level {level}");
    }
}
