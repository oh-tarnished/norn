//! `[mcp]` and `--mcp`: off by default, every transport parsed, a non-loopback
//! address and an unknown key refused.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use norn_server::config::{Invocation, parse_args};
use norn_server::{DaemonConfig, McpTransport};

#[test]
fn mcp_is_off_unless_asked_for() {
    let config = DaemonConfig::default();
    assert!(!config.mcp_settings().enabled);
    assert!(config.mcp_settings().served().is_empty());
    let config = DaemonConfig::from_toml("[mcp]\ntransports = [\"stdio\"]\n").unwrap();
    assert!(
        config.mcp_settings().served().is_empty(),
        "named but not enabled"
    );
}

#[test]
fn every_transport_parses_and_enabled_with_none_is_stdio() {
    let text = r#"
[mcp]
enabled = true
transports = ["stdio", "http:127.0.0.1:8090", "unix:/tmp/m.sock", 'pipe:\\.\pipe\m']
hosts = ["tunnel.example"]
"#;
    let mcp = DaemonConfig::from_toml(text)
        .unwrap()
        .mcp_settings()
        .clone();
    assert_eq!(
        mcp.served(),
        [
            McpTransport::Stdio,
            McpTransport::Http("127.0.0.1:8090".parse().unwrap()),
            McpTransport::Unix("/tmp/m.sock".into()),
            McpTransport::Pipe(r"\\.\pipe\m".into()),
        ]
    );
    assert_eq!(mcp.hosts, ["tunnel.example"]);
    let bare = DaemonConfig::from_toml("[mcp]\nenabled = true\n").unwrap();
    assert_eq!(bare.mcp_settings().served(), [McpTransport::Stdio]);
}

#[test]
fn a_non_loopback_address_or_an_unknown_key_is_refused() {
    let wide = "[mcp]\nenabled = true\ntransports = [\"http:0.0.0.0:8090\"]\n";
    assert!(
        DaemonConfig::from_toml(wide)
            .unwrap_err()
            .to_string()
            .contains("loopback")
    );
    let typo = "[mcp]\nenable = true\n";
    assert!(DaemonConfig::from_toml(typo).is_err());
    let odd = "[mcp]\nenabled = true\ntransports = [\"carrier-pigeon\"]\n";
    assert!(DaemonConfig::from_toml(odd).is_err());
}

#[test]
fn the_mcp_flag_enables_it_and_repeats() {
    let args = ["--mcp", "stdio", "--mcp", "unix:/tmp/a.sock"].map(Into::into);
    let Invocation::Run(config) = parse_args(args).unwrap() else {
        panic!("a run")
    };
    let served = config.mcp_settings().served();
    assert_eq!(
        served,
        [
            McpTransport::Stdio,
            McpTransport::Unix("/tmp/a.sock".into())
        ]
    );
}
