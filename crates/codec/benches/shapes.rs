//! Codec benchmark on realistic one-day 1 Hz sensor shapes.
//!
//! Run with `cargo bench -p norn-codec --bench shapes`. Prints two Markdown
//! tables: every codec per shape (bytes/sample, encode and decode MB/s), and
//! the measured-best encoding sealed with zstd at levels 3-6 (bytes/sample,
//! seal and open MB/s), beside zstd level 3 on the plain unencoded values.
//! MB/s are of in-memory value bytes (8 per i64/f64, 4 per f32/u32, 1 per
//! bool), so codecs of one shape are directly comparable.
#![allow(
    missing_docs,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic
)]

#[path = "shapes/column.rs"]
mod column;
#[path = "shapes/gen.rs"]
mod gen_;

use std::hint::black_box;
use std::time::Instant;

use column::Col;
use norn_codec::ALL_CODECS;
use norn_codec::compress::{self, Compression, ZstdLevel};

/// Seconds each measurement runs for at least.
const BUDGET: f64 = 0.3;

/// Runs `f` repeatedly for [`BUDGET`] and returns MB/s over `bytes`.
fn rate(bytes: usize, mut f: impl FnMut()) -> f64 {
    let (start, mut iters) = (Instant::now(), 0u32);
    while start.elapsed().as_secs_f64() < BUDGET || iters < 3 {
        f();
        iters += 1;
    }
    bytes as f64 * f64::from(iters) / start.elapsed().as_secs_f64() / 1e6
}

fn shapes() -> Vec<(&'static str, Col)> {
    let tenths = gen_::drift_tenths();
    vec![
        ("flat setpoint (f64)", Col::F64(gen_::flat_setpoint())),
        (
            "temperature drift 0.1 (f64)",
            Col::F64(tenths.iter().map(|d| *d as f64 / 10.0).collect()),
        ),
        (
            "temperature drift 0.1 (f32)",
            Col::F32(tenths.iter().map(|d| *d as f32 / 10.0).collect()),
        ),
        ("noisy analog (f64)", Col::F64(gen_::noisy_analog())),
        ("on/off, rare toggles (bool)", Col::Bool(gen_::on_off())),
        ("multi-state (enum)", Col::Enum(gen_::multi_state())),
        ("energy counter (i64)", Col::Int(gen_::energy_counter())),
        ("1 Hz timestamps, regular", Col::Ts(gen_::regular_ts())),
        (
            "1 Hz timestamps, jitter + gaps",
            Col::Ts(gen_::jittered_ts()),
        ),
    ]
}

fn codec_table(shapes: &[(&str, Col)]) {
    println!("| shape | codec | bytes/sample | encode MB/s | decode MB/s |");
    println!("| --- | --- | ---: | ---: | ---: |");
    for (name, col) in shapes {
        let codecs = ALL_CODECS
            .iter()
            .copied()
            .filter(|c| c.column() == col.column());
        for codec in [None].into_iter().chain(codecs.map(Some)) {
            let mut block = Vec::new();
            let Ok(id) = col.encode(codec, &mut block) else {
                continue;
            };
            assert_eq!(col.decode(&block).unwrap(), col.len());
            let enc = rate(col.raw_bytes(), || {
                let mut b = Vec::with_capacity(block.len());
                black_box(col.encode(codec, &mut b).unwrap());
            });
            let dec = rate(col.raw_bytes(), || {
                black_box(col.decode(&block).unwrap());
            });
            let label = match codec {
                None => format!("**auto: {id:?}**"),
                Some(c) => format!("{c:?}"),
            };
            let bps = block.len() as f64 / col.len() as f64;
            println!("| {name} | {label} | {bps:.4} | {enc:.0} | {dec:.0} |");
        }
    }
}

fn zstd_table(shapes: &[(&str, Col)]) {
    println!(
        "| shape | encoded | +zstd 3 | +zstd 4 | +zstd 5 | +zstd 6 | seal MB/s 3/4/5/6 | open+decode MB/s 3/4/5/6 | zstd 3, plain values |"
    );
    println!("| --- | ---: | ---: | ---: | ---: | ---: | --- | --- | ---: |");
    for (name, col) in shapes {
        let mut block = Vec::new();
        col.encode(None, &mut block).unwrap();
        let per = |n: usize| n as f64 / col.len() as f64;
        let (mut sizes, mut seals, mut opens) = (Vec::new(), Vec::new(), Vec::new());
        for level in 3..=6 {
            let how = Compression::Zstd(ZstdLevel::new(level).unwrap());
            let mut sealed = Vec::new();
            compress::seal(&block, how, &mut sealed).unwrap();
            sizes.push(format!("{:.4}", per(sealed.len())));
            let seal = rate(col.raw_bytes(), || {
                let mut s = Vec::new();
                black_box(compress::seal(&block, how, &mut s).unwrap());
            });
            let open = rate(col.raw_bytes(), || {
                let mut o = Vec::new();
                black_box(compress::open(&sealed, &mut o).unwrap());
                black_box(col.decode(&o).unwrap());
            });
            seals.push(format!("{seal:.0}"));
            opens.push(format!("{open:.0}"));
        }
        let mut plain = Vec::new();
        compress::seal(&col.plain(), Compression::Default, &mut plain).unwrap();
        println!(
            "| {name} | {:.4} | {} | {} | {} | {:.4} |",
            per(block.len()),
            sizes.join(" | "),
            seals.join(" / "),
            opens.join(" / "),
            per(plain.len()),
        );
    }
}

fn main() {
    if !std::env::args().any(|a| a == "--bench") {
        return; // `cargo test --all-targets` runs benches as tests; skip.
    }
    let shapes = shapes();
    println!("## Codecs, {} samples per shape\n", gen_::N);
    codec_table(&shapes);
    println!("\n## Measured-best encoding sealed with zstd (bytes/sample)\n");
    zstd_table(&shapes);
}
