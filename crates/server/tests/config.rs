//! Loading configuration: defaults, the TOML file, and flags over it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::ffi::OsString;
use std::time::Duration;

use norn_core::Config;
use norn_server::config::{Invocation, parse_args};
use norn_server::{ConfigError, DaemonConfig, LogFormat, LogLevel};

fn args(list: &[&str]) -> Vec<OsString> {
    list.iter().map(OsString::from).collect()
}

fn run(list: &[&str]) -> DaemonConfig {
    match parse_args(args(list)).unwrap() {
        Invocation::Run(config) => *config,
        other => panic!("expected a run, got {other:?}"),
    }
}

#[test]
fn no_file_and_no_flags_gives_the_documented_defaults() {
    let config = run(&[]);
    assert_eq!(config.engine(), &Config::default());
    assert_eq!(config.endpoint(), norn_ipc::Endpoint::platform_default());
    assert_eq!(config.drain(), Duration::from_secs(10));
    assert_eq!(config.logging().level, LogLevel::Info);
    assert_eq!(config.logging().format, LogFormat::Text);
}

#[test]
fn a_file_sets_only_the_keys_it_names() {
    let config = DaemonConfig::from_toml(
        r#"
        listen = "tcp:127.0.0.1:7171"
        drain_timeout_ms = 2500
        [log]
        format = "json"
        [limits]
        max_clients = 8
        [memory]
        query_bytes = 1048576
        "#,
    )
    .unwrap();
    assert_eq!(config.endpoint().to_string(), "tcp:127.0.0.1:7171");
    assert_eq!(config.drain(), Duration::from_millis(2500));
    assert_eq!(config.logging().format, LogFormat::Json);
    assert_eq!(config.logging().level, LogLevel::Info);
    assert_eq!(config.engine().limits().max_clients, 8);
    assert_eq!(config.engine().memory().query_bytes, 1_048_576);
    let defaults = Config::default();
    assert_eq!(
        config.engine().memory().ingest_bytes,
        defaults.memory().ingest_bytes
    );
}

#[test]
fn an_unknown_key_is_refused_rather_than_ignored() {
    let err = DaemonConfig::from_toml("[limits]\nmax_client = 8\n").unwrap_err();
    assert!(matches!(err, ConfigError::Parse { .. }), "{err:?}");
}

#[test]
fn an_engine_config_norn_core_rejects_is_refused() {
    let err = DaemonConfig::from_toml("[limits]\nmax_streams = 0\n").unwrap_err();
    assert!(matches!(err, ConfigError::Engine(_)), "{err:?}");
}

#[test]
fn an_unknown_log_level_is_refused() {
    let err = DaemonConfig::from_toml("[log]\nlevel = \"loud\"\n").unwrap_err();
    assert!(matches!(
        err,
        ConfigError::Invalid {
            setting: "log.level",
            ..
        }
    ));
}

#[test]
fn flags_override_the_file() {
    let dir = std::env::temp_dir().join(format!("norn-config-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("norn.toml");
    std::fs::write(
        &path,
        "listen = \"tcp:127.0.0.1:1\"\n[log]\nlevel = \"warn\"\n",
    )
    .unwrap();
    let config = run(&[
        "--config",
        path.to_str().unwrap(),
        "--listen",
        "tcp:127.0.0.1:2",
        "--log-format",
        "json",
    ]);
    std::fs::remove_dir_all(&dir).unwrap();
    assert_eq!(config.endpoint().to_string(), "tcp:127.0.0.1:2");
    assert_eq!(config.logging().level, LogLevel::Warn);
    assert_eq!(config.logging().format, LogFormat::Json);
}

#[test]
fn a_missing_file_or_a_bad_flag_is_a_typed_error() {
    let missing = parse_args(args(&["--config", "/nonexistent/norn.toml"])).unwrap_err();
    assert!(matches!(missing, ConfigError::Read { .. }));
    let unknown = parse_args(args(&["--frobnicate"])).unwrap_err();
    assert!(matches!(unknown, ConfigError::Usage(_)));
    let no_value = parse_args(args(&["--listen"])).unwrap_err();
    assert!(matches!(no_value, ConfigError::Usage(_)));
    let remote = parse_args(args(&["--listen", "tcp:10.0.0.1:7171"])).unwrap_err();
    assert!(matches!(
        remote,
        ConfigError::Invalid {
            setting: "--listen",
            ..
        }
    ));
}

#[test]
fn help_and_version_do_not_run_the_daemon() {
    assert!(matches!(
        parse_args(args(&["--help"])),
        Ok(Invocation::Help)
    ));
    assert!(matches!(parse_args(args(&["-V"])), Ok(Invocation::Version)));
}

#[test]
fn the_configuration_guide_lists_every_key_with_its_true_default() {
    // A Windows checkout may turn the guide's line endings into CRLF.
    let readme = include_str!("../../../docs/guides/configuration.md").replace("\r\n", "\n");
    let (_, rest) = readme.split_once("```toml\n").unwrap();
    let (example, _) = rest.split_once("```").unwrap();
    let config = DaemonConfig::from_toml(example).unwrap();
    assert_eq!(config.engine(), &Config::default());
    assert_eq!(config.drain(), norn_server::config::DEFAULT_DRAIN_TIMEOUT);
    assert_eq!(config.logging(), &norn_server::LogConfig::default());
}

#[test]
fn the_maintenance_section_sets_the_schedule_and_a_bad_window_is_refused() {
    let text = "[maintenance]\nbackground = false\ncompaction_interval_ms = 1000\n\
                compaction_window = \"22:00-06:00\"\n";
    let config = DaemonConfig::from_toml(text).unwrap();
    let schedule = config.engine().maintenance();
    assert!(!schedule.background);
    assert_eq!(schedule.compaction_interval, Duration::from_secs(1));
    assert_eq!(schedule.compaction_window, Some((22 * 60, 6 * 60)));
    assert!(schedule.in_window(23 * 60) && schedule.in_window(60));
    assert!(!schedule.in_window(12 * 60));
    let bad = "[maintenance]\ncompaction_window = \"25:00-06:00\"\n";
    assert!(matches!(
        DaemonConfig::from_toml(bad),
        Err(ConfigError::Invalid { .. })
    ));
    let zero = "[maintenance]\nretention_interval_ms = 0\n";
    assert!(DaemonConfig::from_toml(zero).is_err());
}
