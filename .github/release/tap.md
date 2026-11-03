<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Shokki Engineering Homebrew tap

Prebuilt Norn and BACnet tools for macOS and Linux. Formulae are published by
their release workflows; a package becomes installable after its first release.

| Formula | Provides |
| --- | --- |
| `norn` | Norn 0.1.0: the `norn` daemon, `nornctl`, and a user service |
| `bacnet-norn` | The separate BACnet capture binary with a dependency on Norn |
| `bacnet` | The standalone BACnet toolkit, without Norn client dependencies |

## Install Norn

For a private tap, authenticate Git over SSH. Private release downloads also
need a GitHub token from an account with access to the release repository:

```sh
brew tap --custom-remote shokki-engineering/tap git@github.com:shokki-engineering/homebrew-tap.git
export HOMEBREW_GITHUB_API_TOKEN="$(gh auth token)"
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The status must contain `SERVING`. Installation alone does not start the daemon.
Run the service without `sudo`; launchd starts it at login on macOS, and Linux
uses a systemd user unit. A fine-grained download token needs Contents read
access to the repository hosting each private release.

## Files and upgrades

- Configuration: `$(brew --prefix)/etc/norn/norn.toml`, preserved on upgrades.
- Data and working directory: `$(brew --prefix)/var/norn`, mode 0700.
- Socket: `$(brew --prefix)/var/run/norn/norn.sock`, in a mode 0700 directory.
- Logs: stdout and stderr both go to `$(brew --prefix)/var/log/norn.log`.

MCP is disabled in the default configuration. Norn has no InfluxDB dependency.
The service restarts after a crash. Upgrade and restart explicitly:

```sh
brew upgrade norn
brew services restart norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

## BACnet capture

After the `bacnet-norn` formula has been published by bacnet-rs's release workflow:

```sh
brew install shokki-engineering/tap/bacnet-norn
```

Edit `$(brew --prefix)/etc/bacnet-norn/server.toml` for the site, then start
`bacnet-norn server --config "$(brew --prefix)/etc/bacnet-norn/server.toml"`.
Its default Norn endpoint is filled from the local Homebrew prefix, and it
configures no InfluxDB destination. Capture holds readings in its bounded buffer
during Norn restarts; check `bacnet-norn capture sink` for held, delivered and
dropped counts.

## Release automation

Norn's release workflow can initialize this repository even when it is empty.
It needs `HOMEBREW_TAP_TOKEN` with Contents write access to this repository.
The source is [shokki-engineering/norn](https://github.com/shokki-engineering/norn).
See its [Homebrew guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/homebrew.md)
for authenticated curl downloads, configuration and release setup.
