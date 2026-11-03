//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Names, metadata and descriptors.

use norn_core::{
    EntityDescriptor, EntityId, Error, Limit, Metadata, MetadataLimits, Name, NameError, Quality,
    Sample, Sequence, SignalDescriptor, SignalId, Timestamp, Value, ValueKind,
};

#[test]
fn a_name_must_be_non_empty_bounded_and_printable() {
    assert_eq!(Name::new("AHU-01").unwrap().as_str(), "AHU-01");
    let reason = |s: &str| match Name::new(s) {
        Err(Error::InvalidName { reason }) => reason,
        other => panic!("expected a refusal, got {other:?}"),
    };
    assert_eq!(reason(""), NameError::Empty);
    assert_eq!(reason(&"x".repeat(256)), NameError::TooLong(256));
    assert_eq!(reason("a\nb"), NameError::ControlCharacter(1));
    assert!(Name::new("x".repeat(Name::MAX_BYTES)).is_ok());
}

#[test]
fn metadata_refuses_entries_past_its_limits_and_stays_unchanged() {
    let limits = MetadataLimits {
        max_entries: 1,
        max_key_bytes: 4,
        max_value_bytes: 4,
    };
    let mut m = Metadata::with_limits(limits);
    m.insert("unit", "degC").unwrap();
    let err = m.insert("zone", "1").unwrap_err();
    assert_eq!(
        err,
        Error::LimitExceeded {
            limit: Limit::MetadataEntries,
            max: 1,
            actual: 2
        }
    );
    assert!(matches!(
        m.insert("unit", "kelvin"),
        Err(Error::LimitExceeded {
            limit: Limit::MetadataValueBytes,
            ..
        })
    ));
    assert!(matches!(
        m.insert("units", "K"),
        Err(Error::LimitExceeded {
            limit: Limit::MetadataKeyBytes,
            ..
        })
    ));
    assert_eq!(m.get("unit"), Some("degC"));
    assert_eq!(
        m.insert("unit", "K").unwrap(),
        Some("degC".into()),
        "replacing is not growing"
    );
}

#[test]
fn an_adapter_describes_its_domain_only_through_metadata() {
    let entity = EntityDescriptor::new(EntityId::new(1), Name::new("AHU-01").unwrap());
    let mut signal = SignalDescriptor::new(
        SignalId::new(7),
        entity.id,
        Name::new("SupplyAirTemperature").unwrap(),
        ValueKind::F64,
    );
    signal.metadata.insert("protocol", "BACnet").unwrap();
    signal
        .metadata
        .insert("object_type", "analogInput")
        .unwrap();
    let keys: Vec<_> = signal.metadata.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, ["object_type", "protocol"], "iteration is ordered");
    let sample = Sample::new(
        Timestamp::from_nanos(5),
        signal.id,
        Value::F64(21.5),
        Quality::GOOD,
        Sequence::ZERO,
    );
    assert_eq!(sample.value.kind(), signal.kind);
}
