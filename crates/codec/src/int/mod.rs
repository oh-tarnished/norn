//! The `i64` codec family shared by timestamp and integer columns.
//!
//! Selection is by measurement: [`encode_auto`] computes the exact payload
//! size of every applicable algorithm (cheap: no output is written) and keeps
//! the smallest, preferring the earlier of [`CANDIDATES`] on a tie, so a
//! perfectly regular series always becomes start + step.

pub(crate) mod algo;
pub(crate) mod pack;
pub(crate) mod plan;

use algo::Algo;

use crate::error::{CodecError, Result};
use crate::header;
use crate::id::{CodecId, ColumnType};
use crate::wire::Cursor;

/// Algorithms in tie-break order.
const CANDIDATES: [Algo; 5] = [
    Algo::Regular,
    Algo::DeltaOfDelta,
    Algo::Delta,
    Algo::For,
    Algo::Rle,
];

/// The algorithm behind an `i64` codec id.
fn algo_of(codec: CodecId) -> Option<Algo> {
    use CodecId::*;
    Some(match codec {
        TimestampRegular | IntegerRegular => Algo::Regular,
        TimestampDelta | IntegerDelta => Algo::Delta,
        TimestampDeltaOfDelta | IntegerDeltaOfDelta => Algo::DeltaOfDelta,
        TimestampFor | IntegerFor => Algo::For,
        TimestampRle | IntegerRle => Algo::Rle,
        _ => return None,
    })
}

/// The codec id for `algo` in `column` (Timestamp or Integer).
fn codec_for(column: ColumnType, algo: Algo) -> CodecId {
    use CodecId::*;
    let ts = column == ColumnType::Timestamp;
    match algo {
        Algo::Regular if ts => TimestampRegular,
        Algo::Regular => IntegerRegular,
        Algo::Delta if ts => TimestampDelta,
        Algo::Delta => IntegerDelta,
        Algo::DeltaOfDelta if ts => TimestampDeltaOfDelta,
        Algo::DeltaOfDelta => IntegerDeltaOfDelta,
        Algo::For if ts => TimestampFor,
        Algo::For => IntegerFor,
        Algo::Rle if ts => TimestampRle,
        Algo::Rle => IntegerRle,
    }
}

/// The smallest applicable algorithm for `values`.
///
/// A regular series of two or more values short-circuits: start + step is
/// at most 20 bytes, which no packed form of 2+ values undercuts enough to
/// be worth measuring.
pub(crate) fn choose(values: &[i64]) -> Algo {
    if values.len() >= 2 && algo::regular(values).is_some() {
        return Algo::Regular;
    }
    let sized = CANDIDATES
        .iter()
        .filter_map(|a| algo::size(*a, values).map(|s| (s, *a)));
    sized.min_by_key(|(s, _)| *s).map_or(Algo::For, |(_, a)| a)
}

/// Encodes with the measured-best codec, appending header and payload.
pub(crate) fn encode_auto(
    column: ColumnType,
    values: &[i64],
    out: &mut Vec<u8>,
) -> Result<CodecId> {
    let codec = codec_for(column, choose(values));
    encode_with(column, codec, values, out)?;
    Ok(codec)
}

/// Encodes with a forced codec.
pub(crate) fn encode_with(
    column: ColumnType,
    codec: CodecId,
    values: &[i64],
    out: &mut Vec<u8>,
) -> Result<()> {
    header::check_len(values.len())?;
    header::check_column(codec, column)?;
    let algo = algo_of(codec).ok_or(CodecError::NotApplicable { codec })?;
    let mark = out.len();
    header::write(out, codec, values.len());
    if !algo::encode(algo, values, out) {
        out.truncate(mark);
        return Err(CodecError::NotApplicable { codec });
    }
    Ok(())
}

/// Decodes one block of `column` from `cur`, appending to `out`.
pub(crate) fn decode_from(
    column: ColumnType,
    cur: &mut Cursor<'_>,
    out: &mut Vec<i64>,
) -> Result<()> {
    let h = header::read_for(cur, column)?;
    let algo = algo_of(h.codec).ok_or(CodecError::NotApplicable { codec: h.codec })?;
    algo::decode(algo, cur, h.count, out)
}

/// Decodes a whole block, rejecting trailing bytes.
pub(crate) fn decode(column: ColumnType, block: &[u8], out: &mut Vec<i64>) -> Result<()> {
    let mut cur = Cursor::new(block);
    decode_from(column, &mut cur, out)?;
    cur.finish()
}
