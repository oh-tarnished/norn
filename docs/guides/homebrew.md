<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

# Homebrew service

Norn's release workflow generates `norn.rb` after uploading the macOS and
Linux archives. It installs both `norn` and `nornctl`. Until the first release
has published that formula, installation from the tap is unavailable.

## Install and start

For a private tap or private release downloads, use an account that can read both:

```sh
brew tap --custom-remote shokki-engineering/tap git@github.com:shokki-engineering/homebrew-tap.git
export HOMEBREW_GITHUB_API_TOKEN="$(gh auth token)"
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The status must contain `SERVING`. Installing the package alone does not start
it. Run `brew services` as your Homebrew user, without `sudo`: on macOS the
service runs under launchd at login; on Linux it uses a systemd user unit.

SSH authenticates the tap. The token separately authenticates archive downloads
using the same GitHub asset API and curl headers as bacnet-rs `a534df3`. A
read-only token needs Contents access to `shokki-engineering/norn`. Export it
again before an upgrade. The existing bacnet `.netrc` method also works when
enabled with `HOMEBREW_CURLRC`; `.netrc` alone does not enable curl's config.

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

## Release and organization move

The source repository is `shokki-engineering/norn`. Update local clones
with `git remote set-url origin https://github.com/shokki-engineering/norn.git`.
Grant release automation and read-only download tokens access to the new owner.
The formula generator uses `GITHUB_REPOSITORY`, so release asset URLs follow the
repository that runs the workflow.

Set the repository secret `HOMEBREW_TAP_TOKEN` to a token with Contents write
access to the tap. The release workflow uploads `norn.rb` to the release and
updates `Formula/norn.rb`. Without that secret the formula remains available
for a manual tap update. If the tap moves too, set the repository variable
`HOMEBREW_TAP_REPOSITORY` to its new `owner/homebrew-tap` name.

To regenerate a formula, put the release's checksum files and its API metadata
in one directory, then run:

```sh
gh api repos/shokki-engineering/norn/releases/tags/v0.1.0 > dist/release.json
bash .github/release/formula.sh 0.1.0 dist v0.1.0 > dist/norn.rb
```

The renderer refuses missing or duplicate platform archives, malformed
checksums, and digests that disagree with the uploaded assets.
