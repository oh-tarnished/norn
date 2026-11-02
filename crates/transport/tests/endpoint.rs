//! Endpoint syntax: parsing, rejection and round-tripping.
#![allow(clippy::unwrap_used, clippy::panic)]

use norn_transport::{Endpoint, ParseEndpointError, TransportError, bind};

#[test]
fn every_scheme_round_trips_through_display() {
    for text in [
        "unix:/run/user/1000/norn/norn.sock",
        r"pipe:\\.\pipe\norn",
        "tcp:127.0.0.1:7171",
        "tcp:[::1]:7171",
    ] {
        let ep: Endpoint = text.parse().unwrap();
        assert_eq!(ep.to_string(), text);
    }
}

#[test]
fn a_relative_unix_path_is_rejected() {
    let err = "unix:norn.sock".parse::<Endpoint>().unwrap_err();
    assert!(matches!(err, ParseEndpointError::RelativePath(_)));
}

#[test]
fn a_remote_or_empty_pipe_name_is_rejected() {
    for text in [r"pipe:\\server\pipe\norn", r"pipe:\\.\pipe\", "pipe:norn"] {
        let err = text.parse::<Endpoint>().unwrap_err();
        assert!(matches!(err, ParseEndpointError::BadPipeName(_)), "{text}");
    }
}

#[test]
fn a_non_loopback_tcp_address_is_rejected() {
    let err = "tcp:0.0.0.0:7171".parse::<Endpoint>().unwrap_err();
    assert!(matches!(err, ParseEndpointError::NotLoopback(_)));
}

#[test]
fn an_unknown_scheme_is_rejected() {
    let err = "http://localhost".parse::<Endpoint>().unwrap_err();
    assert!(matches!(err, ParseEndpointError::UnknownScheme(_)));
}

#[tokio::test]
async fn binding_a_non_loopback_address_built_in_code_is_refused() {
    let ep = Endpoint::Tcp("0.0.0.0:0".parse().unwrap());
    let err = bind(&ep).await.err().unwrap();
    assert!(matches!(err, TransportError::NotLoopback(_)));
}
