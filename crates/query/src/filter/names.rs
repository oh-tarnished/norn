//! The filter language's words for quality: the protocol's enum value names.
//!
//! They live here, as text, because they are part of the query language and this
//! crate may not import the protocol. `crates/server/tests` holds them equal to
//! the generated enums.

use norn_core::{Severity, Substatus};

/// The severity named `word`.
pub(super) fn severity(word: &str) -> Option<Severity> {
    Some(match word {
        "QUALITY_SEVERITY_GOOD" => Severity::Good,
        "QUALITY_SEVERITY_UNCERTAIN" => Severity::Uncertain,
        "QUALITY_SEVERITY_BAD" => Severity::Bad,
        _ => return None,
    })
}

/// The substatus named `word`.
pub(super) fn substatus(word: &str) -> Option<Substatus> {
    Some(match word {
        "QUALITY_SUBSTATUS_UNSPECIFIED" => Substatus::None,
        "QUALITY_SUBSTATUS_LAST_KNOWN_VALUE" => Substatus::LastKnownValue,
        "QUALITY_SUBSTATUS_STALE" => Substatus::Stale,
        "QUALITY_SUBSTATUS_SUBSTITUTED" => Substatus::Substituted,
        "QUALITY_SUBSTATUS_OVERRIDDEN" => Substatus::Overridden,
        "QUALITY_SUBSTATUS_ESTIMATED" => Substatus::Estimated,
        "QUALITY_SUBSTATUS_OUT_OF_SERVICE" => Substatus::OutOfService,
        "QUALITY_SUBSTATUS_COMMUNICATION_FAILURE" => Substatus::CommunicationFailure,
        "QUALITY_SUBSTATUS_DEVICE_FAILURE" => Substatus::DeviceFailure,
        "QUALITY_SUBSTATUS_SENSOR_FAILURE" => Substatus::SensorFailure,
        "QUALITY_SUBSTATUS_OUT_OF_RANGE" => Substatus::OutOfRange,
        "QUALITY_SUBSTATUS_CONFIG_ERROR" => Substatus::ConfigurationError,
        _ => return None,
    })
}
