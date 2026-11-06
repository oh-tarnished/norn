<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

### norn v1.0.0-rc.2

The second release candidate for norn 1.0. Every platform archive contains the
`norn` daemon, the `nornctl` client, the built-in MCP server and documentation.
All workspace crates, including `norn-protocol`, use version `1.0.0-rc.2`.
Wire negotiation uses protocol 1.8; client and daemon agree the lower minor, so
v1.0.0-rc.1 (1.7) and v0.1.0 (1.6) clients and daemons still connect.

This release adds `nornctl backup`, `restore` and `migrate` with a versioned store
and a [compatibility policy](https://github.com/shokki-engineering/norn/blob/v1.0.0-rc.2/docs/reference/compatibility.md)
stating what 1.x promises; published message limits and `nornctl inspect`, `stats`
and `benchmark`; `norn-sink`, a kit for device adapters; 26 fuzz targets run nightly,
with five fixes; the benchmark matrix; the first security review's fixes; and opt-in
encryption at rest. Encryption at rest is **experimental** in 1.0: it has had no
independent review, and its on-disk form may change within 1.x.

Known issues. `listen = "tcp:..."` and MCP over HTTP, when turned on, are open to
every local user, and the daemon only warns at start; use the Unix socket, a named
pipe or MCP over stdio instead. A maximum-speed replay of a recent window may stop
after `RUNNING`; this remains under investigation. A backup of an encrypted store
that holds no segment yet does not record that it was encrypted, so restored and
started without a key it opens as plain.
See the [v1.0.0-rc.2 changelog](https://github.com/shokki-engineering/norn/blob/v1.0.0-rc.2/docs/project/changelog.md).

`norn.rb` installs the complete program from these archives and supplies a
Homebrew service. See the [Homebrew guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/homebrew.md)
for tap authentication, `brew services start norn`, configuration and health checks.

```sh
brew install shokki-engineering/tap/norn
brew services start norn
nornctl --endpoint "unix:$(brew --prefix)/var/run/norn/norn.sock" status
```

The daemon runs in the background and starts at login. The release workflow
updates the Homebrew tap as Walle. Installation requires no GitHub token.

Download the archive for your platform and verify it against its `.sha256` file.
Extract it, put the binaries on your `PATH`, then run:

```sh
norn --help
nornctl --help
norn
```

In another terminal:

```sh
nornctl status
nornctl streams create example
```

Linux and macOS archives use `.tar.gz`; Windows archives use `.zip` and contain
`norn.exe` and `nornctl.exe`. Linux binaries require glibc 2.35 or newer.

See the included docs, the [getting started guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/getting-started.md)
the [configuration reference](https://github.com/shokki-engineering/norn/blob/main/docs/guides/configuration.md), the
[AI agents guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/ai-agents.md) for `norn --mcp`, or the
[online documentation](https://github.com/shokki-engineering/norn#contents).

Norn is [AGPL-3.0-or-later](https://github.com/shokki-engineering/norn/blob/main/LICENSE) with a separate
[commercial licence](https://github.com/shokki-engineering/norn/blob/main/LICENSE-COMMERCIAL.md), every part of it with no
exception. Licence files accompany each archive.
The source for each release is available in GitHub’s source archives below.
