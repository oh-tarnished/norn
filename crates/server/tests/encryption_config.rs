//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! `[storage] encryption`: what is accepted, and no way to put a key in the TOML.
#![allow(clippy::unwrap_used)]

use norn_server::DaemonConfig;
use norn_server::config::KeySource;

const A: &str = "aa00000000000000000000000000000000000000000000000000000000000000";

#[test]
fn encryption_settings_are_checked_and_a_key_in_the_toml_is_unknown() {
    let bad = |text: &str| DaemonConfig::from_toml(text).unwrap_err().to_string();
    assert!(bad("[storage]\nencryption = \"on\"\n").contains("exactly one"));
    assert!(bad("[storage]\nencryption = \"maybe\"\n").contains("off"));
    assert!(bad("[storage]\nkey_file = \"/k\"\n").contains("need encryption"));
    assert!(bad(&format!("[storage]\nencryption = \"on\"\nkey = \"{A}\"\n")).contains("unknown"));
    let on = "[storage]\nencryption = \"on\"\nkey_env = \"NORN_KEY\"\n";
    let parsed = DaemonConfig::from_toml(on).unwrap();
    assert_eq!(
        parsed.key_source(),
        Some(&KeySource::Env("NORN_KEY".into()))
    );
    assert_eq!(DaemonConfig::default().key_source(), None);
}
