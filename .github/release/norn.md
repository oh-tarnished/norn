<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

### norn v1.0.0-rc.1

The first release candidate for norn 1.0. Every platform archive contains the
`norn` daemon, the `nornctl` client, the built-in MCP server and documentation.
All workspace crates, including `norn-protocol`, use version `1.0.0-rc.1`.
Wire negotiation uses protocol 1.7 and remains compatible with v0.1.0 clients.

This release adds an opt-in shared-memory data plane over iceoryx2 for local
Unix-socket clients, with transparent gRPC fallback and the same acknowledgements,
validation and backpressure. It also adds sealing by open time, verification of
open-segment chunks, and prompt shutdown while append calls remain open.
Shared memory is off by default; enable it only when all processes of the daemon's
user are trusted. A maximum-speed replay of a recent window may stop after
`RUNNING`; this known issue remains under investigation.
See the [v1.0.0-rc.1 changelog](https://github.com/shokki-engineering/norn/blob/v1.0.0-rc.1/docs/project/changelog.md).

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
