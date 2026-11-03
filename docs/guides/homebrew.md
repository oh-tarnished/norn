<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Homebrew service

Norn 0.1.0 ships the `norn` daemon, the `nornctl` client, all interchange
adapters and the built-in MCP server in one package. The release workflow
publishes the macOS and Linux archives and updates `Formula/norn.rb` in
`shokki-engineering/homebrew-tap` as **Walle**.

## Install and start

The tap and release archives are public; installation needs no GitHub token:

```sh
brew tap shokki-engineering/tap
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The status must contain `SERVING`. Installing the package alone does not start
it. Run `brew services` as your Homebrew user, without `sudo`: on macOS the
service runs under launchd at login; on Linux it uses a systemd user unit.

`brew services start norn` starts the background daemon now and registers it
to start at login on your laptop. `brew services list` shows its status;
`brew services stop norn` stops it and removes the login registration.

If the repositories become private, authenticate the tap over SSH with
`brew tap --custom-remote shokki-engineering/tap git@github.com:shokki-engineering/homebrew-tap.git`
and export `HOMEBREW_GITHUB_API_TOKEN` from an account with Contents read
access to `shokki-engineering/norn`. The formula supports authenticated
asset downloads. Export the download token again before upgrades.

## Files and configuration

All paths are resolved from the Homebrew prefix during installation:

| File or directory | Purpose |
| --- | --- |
| `etc/norn/norn.toml` | Editable configuration, preserved on upgrades |
| `var/norn` | Data and service working directory, mode 0700 |
| `var/run/norn/norn.sock` | gRPC socket, in a mode 0700 directory |
| `var/log/norn.log` | stdout and stderr, mode 0600 |

For example, Apple silicon uses `/opt/homebrew/var/run/norn/norn.sock`.
Intel macOS and Linux use their own prefix. The formula generates this config:

```toml
listen = "unix:<Homebrew prefix>/var/run/norn/norn.sock"
data_dir = "<Homebrew prefix>/var/norn"

[mcp]
enabled = false
```

Norn has no InfluxDB listener or exporter. InfluxDB capture belongs to bacnet;
the `bacnet-norn` package's default capture configuration omits it.

The service passes only `--config`, so an edit to `listen` or `data_dir` takes
effect on restart. If you change the socket, change bacnet's `[capture] endpoint`
too. `keep_alive` restarts Norn after a crash.

## Upgrade

```sh
brew upgrade norn
brew services restart norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
bacnet capture sink
```

Norn drains calls on SIGTERM. bacnet-norn holds readings in its configured
bounded buffer while Norn restarts, then reconnects and resumes. Monitor the
capture sink's held, delivered and dropped counts during the restart.

## Release automation and Walle

The source repository is `shokki-engineering/norn`. Create a fine-grained
personal access token with resource owner `shokki-engineering`, repository
access restricted to `homebrew-tap`, and **Contents: read and write**.
GitHub also grants Metadata read access. Complete organization approval if
required. Store it without putting its value in a command or Git:

```sh
gh secret set HOMEBREW_TAP_TOKEN --repo shokki-engineering/norn
```

Paste the PAT at the hidden prompt. The built-in `GITHUB_TOKEN` publishes
Norn's release; this separate PAT writes the cross-repository tap update.
The workflow checks the PAT's tap write access before building, and fails
if it is absent or unusable. Initialize an empty tap with a README before
the first release. Renew the PAT before its expiry using the same command.

Tap commits use author and committer **Walle** (`walle@shokki.engineering`).
This is a Git commit identity; the PAT authenticates as the account that
created it. A separate GitHub bot account is not required. The repository
variable `HOMEBREW_TAP_REPOSITORY` can override the default
`shokki-engineering/homebrew-tap` if the tap moves.

For v0.1.0, review the [changelog](../project/changelog.md), let CI pass on
the release commit, then tag that commit:

```sh
git tag -a v0.1.0 -m "Norn 0.1.0"
git push origin v0.1.0
```

The Release workflow builds five platform archives with SHA-256 checksums,
uploads the formula, and commits it to the tap. Check that **Update the
Homebrew tap as Walle** succeeds, then install and start the service using
the commands above. A manual run checks readiness without publishing by
default; set its `publish` input to `true` to publish the workspace version.

To regenerate a formula, put the release's checksum files and its API metadata
in one directory, then run:

```sh
gh api repos/shokki-engineering/norn/releases/tags/v0.1.0 > dist/release.json
bash .github/release/formula.sh 0.1.0 dist v0.1.0 > dist/norn.rb
```

The renderer refuses missing or duplicate platform archives, malformed
checksums, and digests that disagree with the uploaded assets.
