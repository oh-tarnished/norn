//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! One logged mutation, and its byte form.

use std::time::Duration;

use norn_core::{
    EntityDescriptor, EntityId, SignalDescriptor, SignalId, StreamDescriptor, StreamId, ValueKind,
};

use crate::codec::{In, Out};
use crate::record::{EntityRecord, Retention, SignalRecord, StreamRecord};

/// What one log record says happened. A snapshot is the same records: a put for
/// everything alive, then the ID counters.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Op {
    PutEntity(EntityRecord),
    DeleteEntity(EntityId),
    PutSignal(SignalRecord),
    DeleteSignal(SignalId),
    PutStream(StreamRecord),
    DeleteStream(StreamId),
    NextIds([u64; 3]),
    /// The next `n` records are one batch: replay applies all or none (log v2).
    Begin(u64),
}

impl Op {
    pub(crate) fn encode(&self) -> Vec<u8> {
        let mut o = Out::default();
        match self {
            Self::PutEntity(r) => {
                o.u8(1);
                o.u64(r.descriptor.id.get());
                o.str(r.descriptor.name.as_str());
                o.map(&r.descriptor.metadata);
                o.str(&r.display_name);
                o.time(r.create_time);
                o.time(r.update_time);
                o.u64(r.revision);
            }
            Self::DeleteEntity(id) => {
                o.u8(2);
                o.u64(id.get());
            }
            Self::PutSignal(r) => {
                o.u8(3);
                o.u64(r.descriptor.id.get());
                o.u64(r.descriptor.entity.get());
                o.str(r.descriptor.name.as_str());
                o.u8(r.descriptor.kind.tag());
                o.map(&r.descriptor.metadata);
                o.u64(r.stream.get());
                o.str(&r.display_name);
                o.str(&r.unit);
                o.time(r.create_time);
                o.time(r.update_time);
                o.u64(r.revision);
            }
            Self::DeleteSignal(id) => {
                o.u8(4);
                o.u64(id.get());
            }
            Self::PutStream(r) => {
                o.u8(5);
                o.u64(r.descriptor.id.get());
                o.str(r.descriptor.name.as_str());
                o.map(&r.descriptor.metadata);
                o.str(&r.display_name);
                encode_retention(&mut o, r.retention);
                o.time(r.create_time);
                o.time(r.update_time);
                o.u64(r.revision);
            }
            Self::DeleteStream(id) => {
                o.u8(6);
                o.u64(id.get());
            }
            Self::NextIds(ids) => {
                o.u8(7);
                ids.iter().for_each(|i| o.u64(*i));
            }
            Self::Begin(n) => {
                o.u8(8);
                o.u64(*n);
            }
        }
        o.0
    }

    /// Decodes a record that passed its checksum; `None` if it is not one this
    /// build understands. Copies out of `bytes`.
    pub(crate) fn decode(bytes: &[u8]) -> Option<Self> {
        let mut i = In(bytes);
        Some(match i.u8()? {
            1 => {
                let (id, name) = (EntityId::new(i.u64()?), i.name()?);
                let mut descriptor = EntityDescriptor::new(id, name);
                descriptor.metadata = i.map()?;
                Self::PutEntity(EntityRecord {
                    descriptor,
                    display_name: i.str()?,
                    create_time: i.time()?,
                    update_time: i.time()?,
                    revision: i.u64()?,
                })
            }
            2 => Self::DeleteEntity(EntityId::new(i.u64()?)),
            3 => {
                let (id, entity) = (SignalId::new(i.u64()?), EntityId::new(i.u64()?));
                let name = i.name()?;
                let kind = ValueKind::from_tag(i.u8()?)?;
                let mut descriptor = SignalDescriptor::new(id, entity, name, kind);
                descriptor.metadata = i.map()?;
                Self::PutSignal(SignalRecord {
                    descriptor,
                    stream: StreamId::new(i.u64()?),
                    display_name: i.str()?,
                    unit: i.str()?,
                    create_time: i.time()?,
                    update_time: i.time()?,
                    revision: i.u64()?,
                })
            }
            4 => Self::DeleteSignal(SignalId::new(i.u64()?)),
            5 => {
                let (id, name) = (StreamId::new(i.u64()?), i.name()?);
                let mut descriptor = StreamDescriptor::new(id, name);
                descriptor.metadata = i.map()?;
                Self::PutStream(StreamRecord {
                    descriptor,
                    display_name: i.str()?,
                    retention: decode_retention(&mut i)?,
                    create_time: i.time()?,
                    update_time: i.time()?,
                    revision: i.u64()?,
                })
            }
            6 => Self::DeleteStream(StreamId::new(i.u64()?)),
            7 => Self::NextIds([i.u64()?, i.u64()?, i.u64()?]),
            8 => Self::Begin(i.u64()?),
            _ => return None,
        })
    }
}

fn encode_retention(o: &mut Out, r: Retention) {
    match r {
        Retention::MaxAge(age) => {
            o.u8(1);
            o.u64(u64::try_from(age.as_nanos()).unwrap_or(u64::MAX));
        }
        Retention::MaxBytes(n) => {
            o.u8(2);
            o.u64(n);
        }
        Retention::MaxMonths(n) => {
            o.u8(3);
            o.u64(u64::from(n));
        }
        _ => o.u8(0),
    }
}

fn decode_retention(i: &mut In<'_>) -> Option<Retention> {
    Some(match i.u8()? {
        0 => Retention::Forever,
        1 => Retention::MaxAge(Duration::from_nanos(i.u64()?)),
        2 => Retention::MaxBytes(i.u64()?),
        3 => Retention::MaxMonths(u32::try_from(i.u64()?).ok()?),
        _ => return None,
    })
}
