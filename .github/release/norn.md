<!--
SPDX-FileCopyrightText: 2026 Srikanth K
SPDX-License-Identifier: AGPL-3.0-or-later
-->

### Norn

A pure-Rust temporal data engine. Each platform archive contains the `norn`
daemon, which also serves MCP to AI agents when asked, the `nornctl` client,
and the project documentation.

`norn.rb` installs both binaries from these private archives and supplies a
Homebrew service. See the [Homebrew guide](https://github.com/shokki-engineering/norn/blob/main/docs/guides/homebrew.md)
for tap authentication, `brew services start norn`, configuration and health checks.

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
