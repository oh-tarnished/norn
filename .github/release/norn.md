<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

### Norn 0.1.0 — first release

A pure-Rust temporal data engine. Each platform archive contains the `norn`
daemon, which also serves MCP to AI agents when asked, the `nornctl` client,
and the project documentation.

This first release includes durable ingestion, compressed and checksummed
storage, range queries and aggregates, state at an instant, replay and live
subscriptions, compaction and retention, and CSV, Parquet, Arrow IPC and MCAP
interchange. Seventeen read-only MCP tools are built into the daemon.
See the [v0.1.0 changelog](https://github.com/shokki-engineering/norn/blob/v0.1.0/docs/project/changelog.md).

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
