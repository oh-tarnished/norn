//! Protocol version negotiation, as docs/reference/protocol.md specifies it.

use norn_protocol::{PROTOCOL_VERSION, ProtocolVersion, admin};

const fn v(major: i32, minor: i32) -> ProtocolVersion {
    ProtocolVersion { major, minor }
}

#[test]
fn peers_of_one_major_agree_on_the_lower_minor() {
    assert_eq!(v(1, 3).negotiate(v(1, 1)), Some(v(1, 1)));
    assert_eq!(v(1, 1).negotiate(v(1, 3)), Some(v(1, 1)));
}

#[test]
fn peers_of_different_majors_cannot_agree() {
    assert_eq!(v(1, 0).negotiate(v(2, 0)), None);
}

#[test]
fn the_crate_speaks_the_v1_packages() {
    assert_eq!(PROTOCOL_VERSION.major, 1);
}

#[test]
fn a_version_converts_to_its_message_and_back_unchanged() {
    let message = admin::ProtocolVersion::from(v(1, 4));
    assert_eq!((message.major, message.minor), (1, 4));
    assert_eq!(ProtocolVersion::from(message), v(1, 4));
}
