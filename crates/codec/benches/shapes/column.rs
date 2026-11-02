//! A column of any type, dispatched to its codec module.

use norn_codec::{
    CodecId, ColumnType, Result, boolean, enumeration, float32, float64, integer, timestamp,
};

pub enum Col {
    Ts(Vec<i64>),
    Int(Vec<i64>),
    F64(Vec<f64>),
    F32(Vec<f32>),
    Bool(Vec<bool>),
    Enum(Vec<u32>),
}

impl Col {
    pub fn column(&self) -> ColumnType {
        match self {
            Col::Ts(_) => ColumnType::Timestamp,
            Col::Int(_) => ColumnType::Integer,
            Col::F64(_) => ColumnType::Float64,
            Col::F32(_) => ColumnType::Float32,
            Col::Bool(_) => ColumnType::Boolean,
            Col::Enum(_) => ColumnType::Enum,
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Col::Ts(v) | Col::Int(v) => v.len(),
            Col::F64(v) => v.len(),
            Col::F32(v) => v.len(),
            Col::Bool(v) => v.len(),
            Col::Enum(v) => v.len(),
        }
    }

    /// Bytes the values occupy uncompressed in memory: the MB/s base.
    pub fn raw_bytes(&self) -> usize {
        let width = match self {
            Col::Ts(_) | Col::Int(_) | Col::F64(_) => 8,
            Col::F32(_) | Col::Enum(_) => 4,
            Col::Bool(_) => 1,
        };
        self.len() * width
    }

    /// The values as plain little-endian bytes, for zstd-without-encoding.
    pub fn plain(&self) -> Vec<u8> {
        match self {
            Col::Ts(v) | Col::Int(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Col::F64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Col::F32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
            Col::Bool(v) => v.iter().map(|b| u8::from(*b)).collect(),
            Col::Enum(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        }
    }

    /// Encodes with `codec`, or the measured choice when `None`.
    pub fn encode(&self, codec: Option<CodecId>, out: &mut Vec<u8>) -> Result<CodecId> {
        macro_rules! go {
            ($m:ident, $v:expr) => {
                match codec {
                    Some(c) => $m::encode_with(c, $v, out).map(|()| c),
                    None => $m::encode($v, out),
                }
            };
        }
        match self {
            Col::Ts(v) => go!(timestamp, v),
            Col::Int(v) => go!(integer, v),
            Col::F64(v) => go!(float64, v),
            Col::F32(v) => go!(float32, v),
            Col::Bool(v) => go!(boolean, v),
            Col::Enum(v) => go!(enumeration, v),
        }
    }

    /// Decodes `block` as this column's type and returns the value count.
    pub fn decode(&self, block: &[u8]) -> Result<usize> {
        fn n<T>(block: &[u8], f: fn(&[u8], &mut Vec<T>) -> Result<()>) -> Result<usize> {
            let mut out = Vec::new();
            f(block, &mut out).map(|()| out.len())
        }
        match self {
            Col::Ts(_) => n(block, timestamp::decode),
            Col::Int(_) => n(block, integer::decode),
            Col::F64(_) => n(block, float64::decode),
            Col::F32(_) => n(block, float32::decode),
            Col::Bool(_) => n(block, boolean::decode),
            Col::Enum(_) => n(block, enumeration::decode),
        }
    }
}
