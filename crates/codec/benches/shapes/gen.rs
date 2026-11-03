//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Realistic one-day sensor shapes at 1 Hz (86,400 samples), generated
//! deterministically so every run measures identical data.

/// Samples per shape: one day at 1 Hz.
pub const N: usize = 86_400;

/// A deterministic LCG; good enough for shaping noise.
pub struct Lcg(u64);

impl Lcg {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }
    /// Uniform in [0, 1).
    pub fn unit(&mut self) -> f64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    /// Approximately normal (sum of uniforms), mean 0, sd 1.
    pub fn gauss(&mut self) -> f64 {
        (0..12).map(|_| self.unit()).sum::<f64>() - 6.0
    }
}

/// A setpoint that sits at 21.0 and is changed four times a day.
pub fn flat_setpoint() -> Vec<f64> {
    (0..N)
        .map(|i| [21.0, 22.5, 21.0, 19.5][i / (N / 4)])
        .collect()
}

/// A temperature drifting slowly, reported at 0.1 resolution: the
/// value is parsed from decimal tenths, as many devices report it.
pub fn drift_tenths() -> Vec<i64> {
    let mut rng = Lcg::new(1);
    let (mut t, mut out) = (215.0f64, Vec::with_capacity(N));
    for i in 0..N {
        let diurnal = 15.0 * (i as f64 * std::f64::consts::TAU / N as f64).sin();
        t += rng.gauss() * 0.05;
        out.push((t + diurnal).round() as i64);
    }
    out
}

/// Noisy analog: a duct pressure at full sensor precision.
pub fn noisy_analog() -> Vec<f64> {
    let mut rng = Lcg::new(2);
    (0..N)
        .map(|i| 250.0 + 20.0 * (i as f64 / 900.0).sin() + rng.gauss() * 1.7)
        .collect()
}

/// A pump on/off status that toggles about 24 times a day.
pub fn on_off() -> Vec<bool> {
    let mut rng = Lcg::new(3);
    let mut on = false;
    (0..N)
        .map(|_| {
            if rng.unit() < 24.0 / N as f64 {
                on = !on;
            }
            on
        })
        .collect()
}

/// A four-state mode (occupied, unoccupied, standby, override) changing
/// about every 40 minutes.
pub fn multi_state() -> Vec<u32> {
    let mut rng = Lcg::new(4);
    let mut s = 1u32;
    (0..N)
        .map(|_| {
            if rng.unit() < 1.0 / 2400.0 {
                s = (rng.unit() * 4.0) as u32;
            }
            s
        })
        .collect()
}

/// An energy meter in watt-hours: monotonic, variable increments.
pub fn energy_counter() -> Vec<i64> {
    let mut rng = Lcg::new(5);
    let mut e = 48_213_000_000i64;
    (0..N)
        .map(|_| {
            e += (rng.unit() * 12.0) as i64;
            e
        })
        .collect()
}

/// 1 Hz nanosecond timestamps: perfectly regular.
pub fn regular_ts() -> Vec<i64> {
    (0..N as i64)
        .map(|i| 1_761_955_200_000_000_000 + i * 1_000_000_000)
        .collect()
}

/// 1 Hz timestamps with millisecond jitter on 5% of samples and a gap of a
/// few minutes about twice an hour.
pub fn jittered_ts() -> Vec<i64> {
    let mut rng = Lcg::new(6);
    let mut t = 1_761_955_200_000_000_000i64;
    (0..N)
        .map(|_| {
            t += 1_000_000_000;
            if rng.unit() < 1.0 / 1800.0 {
                t += (rng.unit() * 300.0) as i64 * 1_000_000_000;
            }
            let jitter = if rng.unit() < 0.05 {
                (rng.gauss() * 3e6) as i64
            } else {
                0
            };
            t + jitter
        })
        .collect()
}
