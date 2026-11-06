//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! The resolution order, as a table: every fact told, nothing read.

use super::*;

fn platform_default() -> Endpoint {
    Endpoint::Unix(PathBuf::from("/run/user/501/norn/norn.sock"))
}

fn all() -> Facts {
    Facts {
        named: Some("unix:/named.sock".to_owned()),
        env: Some("unix:/env.sock".to_owned()),
        homebrew: Some(PathBuf::from("/opt/homebrew/var/run/norn/norn.sock")),
        platform_default: platform_default(),
    }
}

fn chosen(facts: &Facts) -> (String, Rule) {
    let (endpoint, rule) = resolve(facts).unwrap();
    (endpoint.to_string(), rule)
}

#[test]
fn a_named_endpoint_wins_over_everything_else() {
    assert_eq!(chosen(&all()), ("unix:/named.sock".to_owned(), Rule::Named));
}

#[test]
fn norn_endpoint_comes_next() {
    let f = Facts {
        named: None,
        ..all()
    };
    assert_eq!(chosen(&f), ("unix:/env.sock".to_owned(), Rule::Env));
}

#[test]
fn the_homebrew_socket_comes_after_the_environment() {
    let f = Facts {
        named: None,
        env: None,
        ..all()
    };
    let want = "unix:/opt/homebrew/var/run/norn/norn.sock".to_owned();
    assert_eq!(chosen(&f), (want, Rule::Homebrew));
}

#[test]
fn the_platform_default_is_the_last_resort() {
    let f = Facts {
        named: None,
        env: None,
        homebrew: None,
        ..all()
    };
    let want = platform_default().to_string();
    assert_eq!(chosen(&f), (want, Rule::PlatformDefault));
}

#[test]
fn a_mistyped_endpoint_is_an_error_naming_its_rule_and_not_a_reason_to_go_on() {
    let f = Facts {
        named: Some("/no/scheme.sock".to_owned()),
        ..all()
    };
    assert_eq!(resolve(&f).unwrap_err().rule, Rule::Named);
    let f = Facts {
        named: None,
        env: Some("tcp:10.0.0.5:7171".to_owned()),
        ..all()
    };
    let error = resolve(&f).unwrap_err();
    assert_eq!(error.rule, Rule::Env);
    assert!(matches!(error.source, ParseEndpointError::NotLoopback(_)));
}

#[test]
fn every_scheme_norn_parses_is_taken() {
    for text in [
        "unix:/x/norn.sock",
        r"pipe:\\.\pipe\norn",
        "tcp:127.0.0.1:7171",
    ] {
        let f = Facts {
            named: Some(text.to_owned()),
            ..all()
        };
        assert_eq!(chosen(&f).0, text);
    }
}

#[test]
fn homebrew_prefix_is_tried_first_and_then_the_usual_prefixes_in_order() {
    let only = |hit: &'static str| move |p: &Path| p == Path::new(hit);
    let custom = "/brew/var/run/norn/norn.sock";
    let found = homebrew_socket(Some("/brew"), &only(custom));
    assert_eq!(found.unwrap(), PathBuf::from(custom));
    let intel = "/usr/local/var/run/norn/norn.sock";
    let found = homebrew_socket(Some("/brew"), &only(intel));
    assert_eq!(found.unwrap(), PathBuf::from(intel));
    let linux = "/home/linuxbrew/.linuxbrew/var/run/norn/norn.sock";
    let found = homebrew_socket(None, &only(linux));
    assert_eq!(found.unwrap(), PathBuf::from(linux));
}

#[test]
fn both_apple_silicon_and_intel_prefixes_holding_a_socket_take_apple_silicon() {
    let found = homebrew_socket(None, &|_| true).unwrap();
    assert_eq!(found, PathBuf::from("/opt/homebrew/var/run/norn/norn.sock"));
}

#[test]
fn no_socket_under_any_prefix_is_no_homebrew_endpoint() {
    assert_eq!(homebrew_socket(Some("/brew"), &|_| false), None);
    assert_eq!(homebrew_socket(Some(""), &|_| false), None);
}
