//! SPDX-FileCopyrightText: 2026 Srikanth K
//! SPDX-License-Identifier: AGPL-3.0-or-later

//! Which Norn to talk to, when nothing or something names one. The order,
//! first match wins:
//!
//! 1. an endpoint the adapter names, from its flag or its configuration;
//! 2. `NORN_ENDPOINT`;
//! 3. the Homebrew service's socket, `<prefix>/var/run/norn/norn.sock`
//!    (docs/guides/homebrew.md), for the first prefix holding one:
//!    `HOMEBREW_PREFIX`, then [`HOMEBREW_PREFIXES`]. Found by looking for the
//!    socket, never by running `brew`. Not on Windows;
//! 4. Norn's own default, [`Endpoint::platform_default`], which a `norn`
//!    started by hand with no `--listen` serves on.
//!
//! Every text goes through Norn's own parser, so `unix:/path`,
//! `pipe:\\.\pipe\name` and loopback `tcp:host:port` mean here exactly what
//! they mean to `norn --listen`. [`resolve`] is pure, told every fact; [`facts`]
//! gathers them from this process, and [`locate`] does both.

use norn_ipc::Endpoint;
use norn_transport::ParseEndpointError;
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(test)]
mod tests;

/// The prefixes Homebrew installs under, after `HOMEBREW_PREFIX`: Apple
/// silicon, Intel macOS, Linux.
pub const HOMEBREW_PREFIXES: &[&str] =
    &["/opt/homebrew", "/usr/local", "/home/linuxbrew/.linuxbrew"];

/// The socket under a Homebrew prefix, as Norn's formula configures it.
pub const HOMEBREW_SOCKET: &str = "var/run/norn/norn.sock";

/// Which rule chose an endpoint. Plain data.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Rule {
    /// The adapter named it.
    Named,
    /// The environment's `NORN_ENDPOINT`.
    Env,
    /// The Homebrew service's socket, found under a prefix.
    Homebrew,
    /// Norn's platform default.
    PlatformDefault,
}

impl fmt::Display for Rule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Named => "the adapter's configuration",
            Self::Env => "NORN_ENDPOINT",
            Self::Homebrew => "the Homebrew service socket",
            Self::PlatformDefault => "Norn's platform default",
        })
    }
}

/// A text Norn's parser refuses, and where it came from: a mistyped endpoint is
/// a mistake to fix, never a reason to try the next rule. Plain data.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EndpointError {
    /// The rule that gave the text.
    pub rule: Rule,
    /// Why it was refused.
    pub source: ParseEndpointError,
}

impl fmt::Display for EndpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.rule, self.source)
    }
}

impl std::error::Error for EndpointError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Everything [`resolve`] decides from. Plain data.
#[derive(Clone, Debug)]
pub struct Facts {
    /// An endpoint the adapter named.
    pub named: Option<String>,
    /// `NORN_ENDPOINT`, if set and not empty.
    pub env: Option<String>,
    /// The Homebrew service's socket, if one was found.
    pub homebrew: Option<PathBuf>,
    /// Norn's platform default.
    pub platform_default: Endpoint,
}

/// The endpoint `facts` lead to, and the rule that chose it.
///
/// # Errors
///
/// A named or `NORN_ENDPOINT` text Norn's parser refuses.
pub fn resolve(facts: &Facts) -> Result<(Endpoint, Rule), EndpointError> {
    let parse = |text: &str, rule: Rule| {
        text.parse::<Endpoint>()
            .map(|endpoint| (endpoint, rule))
            .map_err(|source| EndpointError { rule, source })
    };
    if let Some(text) = &facts.named {
        return parse(text, Rule::Named);
    }
    if let Some(text) = &facts.env {
        return parse(text, Rule::Env);
    }
    if let Some(socket) = &facts.homebrew {
        return Ok((Endpoint::Unix(socket.clone()), Rule::Homebrew));
    }
    Ok((facts.platform_default.clone(), Rule::PlatformDefault))
}

/// The first Homebrew prefix holding Norn's socket: `prefix_env` (from
/// `HOMEBREW_PREFIX`) first, then [`HOMEBREW_PREFIXES`]. `exists` is asked of
/// each candidate, so a test needs no file system.
pub fn homebrew_socket(
    prefix_env: Option<&str>,
    exists: &dyn Fn(&Path) -> bool,
) -> Option<PathBuf> {
    let from_env = prefix_env.filter(|p| !p.is_empty());
    from_env
        .into_iter()
        .chain(HOMEBREW_PREFIXES.iter().copied())
        .map(|prefix| Path::new(prefix).join(HOMEBREW_SOCKET))
        .find(|socket| exists(socket))
}

/// The facts of this process for `named`: its environment, and whether a
/// Homebrew socket exists. Reads the environment and asks the file system
/// whether a path exists; opens nothing.
#[must_use]
pub fn facts(named: Option<&str>) -> Facts {
    let var = |name: &str| std::env::var(name).ok().filter(|v| !v.is_empty());
    let homebrew = if cfg!(unix) {
        homebrew_socket(var("HOMEBREW_PREFIX").as_deref(), &|p| p.exists())
    } else {
        None
    };
    Facts {
        named: named.map(str::to_owned),
        env: var("NORN_ENDPOINT"),
        homebrew,
        platform_default: Endpoint::platform_default(),
    }
}

/// [`resolve`] of this process's [`facts`], with `named` from the adapter's
/// flag or configuration.
///
/// # Errors
///
/// As [`resolve`].
pub fn locate(named: Option<&str>) -> Result<(Endpoint, Rule), EndpointError> {
    resolve(&facts(named))
}
