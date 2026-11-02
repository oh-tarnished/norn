//! The five `i64` algorithms behind the timestamp and integer columns.
//!
//! Every derived quantity (delta, second delta, run value) uses wrapping
//! arithmetic, and decoding applies the inverse wrapping operation, so the
//! round trip is exact across the whole `i64` range, extremes included.

use super::pack;
use crate::error::{CodecError, Result};
use crate::wire::{Cursor, put_ivarint, put_varint, zigzag};

/// One `i64` algorithm, independent of which column uses it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Algo {
    Regular,
    Delta,
    DeltaOfDelta,
    For,
    Rle,
}

/// Successive differences, wrapping.
pub(crate) fn deltas(values: &[i64]) -> Vec<i64> {
    values
        .windows(2)
        .map(|w| w.get(1).copied().unwrap_or(0).wrapping_sub(first(w)))
        .collect()
}

fn first(s: &[i64]) -> i64 {
    s.first().copied().unwrap_or(0)
}

/// `(start, step)` if every value is `start + i * step` (wrapping).
pub(crate) fn regular(values: &[i64]) -> Option<(i64, i64)> {
    let start = first(values);
    let step = values.get(1).map_or(0, |v| v.wrapping_sub(start));
    let mut expect = start;
    for v in values {
        if *v != expect {
            return None;
        }
        expect = expect.wrapping_add(step);
    }
    Some((start, step))
}

/// Run values and lengths.
pub(crate) fn runs(values: &[i64]) -> (Vec<i64>, Vec<i64>) {
    let (mut vals, mut lens) = (Vec::new(), Vec::new());
    for chunk in values.chunk_by(|a, b| a == b) {
        vals.push(first(chunk));
        lens.push(chunk.len() as i64);
    }
    (vals, lens)
}

/// Encoded payload size of `algo`, or `None` if it does not apply.
pub(crate) fn size(algo: Algo, values: &[i64]) -> Option<usize> {
    let head = pack::varint_len(zigzag(first(values)));
    Some(match algo {
        Algo::Regular => regular(values)
            .map(|(a, b)| pack::varint_len(zigzag(a)) + pack::varint_len(zigzag(b)))?,
        Algo::Delta => head + pack::size(&deltas(values)),
        Algo::DeltaOfDelta => {
            let d = deltas(values);
            head + pack::varint_len(zigzag(first(&d))) + pack::size(&deltas(&d))
        }
        Algo::For => pack::size(values),
        Algo::Rle => {
            let (v, l) = runs(values);
            pack::varint_len(v.len() as u64) + pack::size(&v) + pack::size(&l)
        }
    })
}

/// Appends the payload of `values` under `algo`; `false` if inapplicable.
pub(crate) fn encode(algo: Algo, values: &[i64], out: &mut Vec<u8>) -> bool {
    if values.is_empty() {
        return true;
    }
    match algo {
        Algo::Regular => match regular(values) {
            Some((start, step)) => {
                put_ivarint(out, start);
                put_ivarint(out, step);
            }
            None => return false,
        },
        Algo::Delta => {
            put_ivarint(out, first(values));
            pack::encode(&deltas(values), out);
        }
        Algo::DeltaOfDelta => {
            put_ivarint(out, first(values));
            let d = deltas(values);
            if let Some(d0) = d.first() {
                put_ivarint(out, *d0);
                pack::encode(&deltas(&d), out);
            }
        }
        Algo::For => pack::encode(values, out),
        Algo::Rle => {
            let (vals, lens) = runs(values);
            put_varint(out, vals.len() as u64);
            pack::encode(&vals, out);
            pack::encode(&lens, out);
        }
    }
    true
}

/// Reads `count` values encoded under `algo`, appending them to `out`.
pub(crate) fn decode(algo: Algo, cur: &mut Cursor<'_>, n: usize, out: &mut Vec<i64>) -> Result<()> {
    if n == 0 {
        return Ok(());
    }
    out.reserve(n);
    match algo {
        Algo::Regular => {
            let (mut v, step) = (cur.ivarint()?, cur.ivarint()?);
            for _ in 0..n {
                out.push(v);
                v = v.wrapping_add(step);
            }
        }
        Algo::Delta => {
            let mut v = cur.ivarint()?;
            out.push(v);
            let mut d = Vec::with_capacity(n - 1);
            pack::decode(cur, n - 1, &mut d)?;
            for dv in d {
                v = v.wrapping_add(dv);
                out.push(v);
            }
        }
        Algo::DeltaOfDelta => {
            let mut v = cur.ivarint()?;
            out.push(v);
            if n > 1 {
                let mut delta = cur.ivarint()?;
                v = v.wrapping_add(delta);
                out.push(v);
                let mut dd = Vec::with_capacity(n - 2);
                pack::decode(cur, n - 2, &mut dd)?;
                for x in dd {
                    delta = delta.wrapping_add(x);
                    v = v.wrapping_add(delta);
                    out.push(v);
                }
            }
        }
        Algo::For => pack::decode(cur, n, out)?,
        Algo::Rle => decode_runs(cur, n, out)?,
    }
    Ok(())
}

fn decode_runs(cur: &mut Cursor<'_>, n: usize, out: &mut Vec<i64>) -> Result<()> {
    let runs = cur.count(n)?;
    let (mut vals, mut lens) = (Vec::with_capacity(runs), Vec::with_capacity(runs));
    pack::decode(cur, runs, &mut vals)?;
    let at = cur.pos();
    pack::decode(cur, runs, &mut lens)?;
    let mut left = n;
    for (v, len) in vals.into_iter().zip(lens) {
        let len = usize::try_from(len).ok().filter(|l| *l >= 1 && *l <= left);
        let len = len.ok_or(CodecError::Corrupt {
            offset: at,
            reason: "run length out of range",
        })?;
        out.extend(core::iter::repeat_n(v, len));
        left -= len;
    }
    match left {
        0 => Ok(()),
        _ => Err(CodecError::Corrupt {
            offset: at,
            reason: "runs do not cover the block",
        }),
    }
}
