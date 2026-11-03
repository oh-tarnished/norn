<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Shokki Engineering Homebrew tap

Prebuilt Norn and BACnet tools for macOS and Linux. Norn's first release is
**v0.1.0**; its release workflow maintains the formula as **Walle**.

| Formula | Provides |
| --- | --- |
| `norn` | Norn 0.1.0: the `norn` daemon, `nornctl`, and a user service |
| `bacnet-norn` | The separate BACnet capture binary with a dependency on Norn |
| `bacnet` | The standalone BACnet toolkit, without Norn client dependencies |

## Install Norn

The tap and Norn archives are public; installation needs no GitHub token:

```sh
brew tap shokki-engineering/tap
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The status must contain `SERVING`. Installation alone does not start the daemon.
Run the service without `sudo`; launchd starts it now and at login on macOS,
and Linux uses a systemd user unit. `brew services list` shows the daemon;
`brew services stop norn` stops it and removes its login registration.
For private release repositories, export `HOMEBREW_GITHUB_API_TOKEN` with
Contents read access to the repository hosting that release.

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

Norn's release workflow uses the `HOMEBREW_TAP_TOKEN` repository secret: a
fine-grained PAT restricted to this tap with Contents read/write access.
Tap commits have author and committer **Walle** (`walle@shokki.engineering`).
The workflow verifies access before building and fails if the PAT is missing,
expired or unable to push. Initialize the tap with this README before its
first automated update, and rotate the PAT before expiry.
The source is [shokki-engineering/norn](https://github.com/shokki-engineering/norn).
See its [Homebrew guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/homebrew.md)
for authenticated curl downloads, configuration and release setup.
