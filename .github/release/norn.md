### Norn

A pure-Rust temporal data engine. Each platform archive contains the `norn`
daemon, the `nornctl` client, and the project documentation.

Download the archive for your platform and verify it against its `.sha256` file.
Extract it, put both binaries on your `PATH`, then run:

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

See the included docs, the [getting started guide](https://github.com/oh-tarnished/norn/blob/main/docs/guides/getting-started.md)
and [configuration reference](https://github.com/oh-tarnished/norn/blob/main/docs/guides/configuration.md), or the
[online documentation](https://github.com/oh-tarnished/norn#contents).

Norn is [AGPL-3.0-or-later](https://github.com/oh-tarnished/norn/blob/main/LICENSE) with a separate
[commercial licence](https://github.com/oh-tarnished/norn/blob/main/LICENSE-COMMERCIAL.md). The protocol definitions
and `norn-protocol` crate are Apache-2.0. Licence files accompany each archive.
The source for each release is available in GitHub’s source archives below.
