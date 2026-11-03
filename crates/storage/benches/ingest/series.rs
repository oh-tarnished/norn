//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The benchmark's signal population, over the codec benchmark's own shapes.

use norn_core::{Quality, Sample, Sequence, Severity, SignalId, Substatus, Timestamp, Value};

use super::gen_;
use gen_::N;

/// Seven value shapes, assigned round-robin, so 1/7 of signals have each.
pub const SHAPES: [&str; 7] = [
    "flat setpoint (f64)",
    "drift 0.1 (f64)",
    "drift 0.1 (f32)",
    "noisy analog (f64)",
    "on/off (bool)",
    "multi-state (enum)",
    "energy counter (i64)",
];

/// One day of each value shape, and of both timestamp shapes.
pub struct Population {
    flat: Vec<f64>,
    tenths: Vec<i64>,
    noisy: Vec<f64>,
    on_off: Vec<bool>,
    modes: Vec<u32>,
    energy: Vec<i64>,
    regular: Vec<i64>,
    jittered: Vec<i64>,
    signals: usize,
}

impl Population {
    pub fn new(signals: usize) -> Self {
        Self {
            flat: gen_::flat_setpoint(),
            tenths: gen_::drift_tenths(),
            noisy: gen_::noisy_analog(),
            on_off: gen_::on_off(),
            modes: gen_::multi_state(),
            energy: gen_::energy_counter(),
            regular: gen_::regular_ts(),
            jittered: gen_::jittered_ts(),
            signals,
        }
    }

    /// Sample `second` of `signal`. Each signal reads its shape at its own
    /// offset into the day, so no two chunks are copies. One signal in five
    /// has jittered timestamps; about 1 sample in 1000 is uncertain. The
    /// sequence is per stream, as the write path will assign it, so one
    /// signal sees a large regular stride.
    pub fn sample(&self, signal: usize, second: usize) -> Sample {
        let at = (signal.wrapping_mul(7_919) + second) % N;
        let v = match signal % SHAPES.len() {
            0 => Value::F64(self.flat[at]),
            1 => Value::F64(self.tenths[at] as f64 / 10.0),
            2 => Value::F32(self.tenths[at] as f32 / 10.0),
            3 => Value::F64(self.noisy[at]),
            4 => Value::Bool(self.on_off[at]),
            5 => Value::Enum(self.modes[at]),
            _ => Value::I64(self.energy[at]),
        };
        let ts = if signal % 5 == 4 {
            &self.jittered
        } else {
            &self.regular
        };
        let quality = if at % 1000 == 999 {
            Quality::new(Severity::Uncertain, Substatus::Stale)
        } else {
            Quality::GOOD
        };
        let seq = (second * self.signals + signal) as u64;
        Sample::new(
            Timestamp::from_nanos(ts[second % N]),
            SignalId::new(signal as u64),
            v,
            quality,
            Sequence::new(seq),
        )
    }
}
