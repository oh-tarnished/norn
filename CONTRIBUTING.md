# Contributing

Read [spec.md](docs/project/spec.md) and [plan.md](docs/project/plan.md) first; they are binding.

## The 200-line rule

Every `.rs`, `.proto`, `.py` and `.sh` file stays **under 200 lines**
(SPEC §2), tests included; `xtask size` fails at 200. Markdown and anything
under a `generated/` directory are exempt. Near the limit, split along a seam
that already exists (encode from decode, a type from its validation), move
tests to `tests/`, or extract a submodule. Never compress code or delete
documentation to fit.

## The gates

`just gates` runs `cargo run -p xtask -- gates`:

| Gate | Fails when |
| --- | --- |
| `size` | a measured file has 200 or more lines |
| `purity` | `crates/core/src` names `std::fs`, `std::io`, `std::net`, `std::env`, `std::process`, `std::thread`, `SystemTime` or `Instant` outside a comment |
| `docs` | a crate root does not open with a `//!` layer contract; a relative link, image or `#anchor` in a Markdown file does not resolve; a file under `docs/` is missing from the README's Contents, or the reverse; a README exists anywhere but the root |
| `arch` | a crate depends outward, or has no row in the layer table |
| `deps` | a shipped dependency graph, on a target Norn builds, reaches C (`links`, `cc`, `cmake`, ...) |

Every gate also fails when it examined nothing, and every gate has a test in
`xtask/tests/` proving it bites. Never weaken a gate to pass: fix the code,
or change the rule in `xtask` deliberately and say why in the commit.

## CI and releases

Install Rust 1.99.1 (see `rust-toolchain.toml`), `just`, Buf 1.75.0, cargo-deny 0.20.2 and
Go, then run `just api-lint-tools gen-tools` to install the pinned protobuf
tools. Make sure Go's bin directory is on `PATH`. `just ci` runs the local
build, lint, tests, benchmark smoke runs, rustdoc, repository gates, dependency
checks, protobuf lint, strict Google AIP lint and generated-code checks.
`just gen-check` compares a fresh generation in a temporary directory, so it
also works before the first commit and does not rewrite the working tree.

GitHub runs the build, lint, tests and rustdoc on Linux, macOS and Windows.
Benchmark smoke runs use Unix sockets, so Windows only compile-checks them.
ARMv7 and AArch64 Linux also get `just cross <target>` checks. The API job
compares the protobuf contract with the PR base or the previous pushed commit
using `just buf-breaking ".git#ref=<base-sha>"`; the first schema commit has
no compatibility baseline.

Before using Dependabot auto-merge, enable repository auto-merge and protect
the default branch with these required checks:

- `Conformance gates`
- `Build, lint and test (ubuntu-latest)`
- `Build, lint and test (macos-latest)`
- `Build, lint and test (windows-latest)`
- `Licences and advisories`
- `Cross-compile (armv7-unknown-linux-gnueabihf)`
- `Cross-compile (aarch64-unknown-linux-gnu)`
- `Protobuf, Google AIP and generated code`

CI accepts both `main` and `master`. Dependabot uses the repository's default
branch. Branch protection and auto-merge settings are configured on GitHub.

A `vX.Y.Z` tag matching every workspace package version, or a manual Release
workflow run, builds `norn`, `nornctl` and `norn-mcp` for Linux x86-64/AArch64, macOS
Intel/Apple Silicon and Windows x86-64. It attaches archives, documentation
and SHA-256 checksums to a GitHub release. Linux builds target glibc 2.35.
Prerelease versions create prereleases. There is no crate or Homebrew publish.
To preview locally, run `just release-build <host-target>` followed by
`just release-package <host-target> <version>`; output goes to `dist/`.

## Licence and contributions

Norn uses AGPL-3.0-or-later with a separate commercial licence, for every part
of the repository with no exception. Read the
[Contributor Licence Agreement](.github/CLA.md) before contributing and sign
off every commit with `git commit -s`.

## Code rules

- Pure Rust, no `unsafe` (workspace lint `unsafe_code = "deny"`).
- No `unwrap`, `expect`, `panic!` or indexing outside tests (workspace clippy
  lints; `clippy.toml` allows them inside `#[test]`).
- Typed, `#[non_exhaustive]` errors carrying context; callers match, never
  compare messages.
- Every bound is explicit and configurable: no unbounded queue, buffer or
  cache (SPEC §16).
- Minimise dependencies (SPEC §39): declare each once in the root
  `Cargo.toml` `[workspace.dependencies]` with why it earns its place.

## Documentation rules

- Every crate root opens with a `//!` layer contract: what it owns, what it
  must not do, what it may import, which spans it opens.
- Every public item has rustdoc: purpose, invariants, concurrency, failure
  behaviour, performance where it matters.
- A function taking `&[u8]` says whether its result borrows or copies.
- Documents describe what exists; a target is labelled as one.
- `README.md`, this file and `SECURITY.md` stay at the root; everything else is
  under `docs/`. Add a file there and add it to the README's Contents.
- Guides paste real output. Images come from a real daemon:
  `tools/screenshots/capture.sh target/debug docs/images`.
- Change the design doc before the code.

## Tests

Test names are sentences: `an_unknown_tag_is_refused_rather_than_misread`.
Lossless codecs get property tests of `decode(encode(x)) == x`.

## How waves merge

Each piece works in its own git worktree on `wave-N/<piece>`, branched from
`main`, and never edits another piece's crate. A change a piece needs in a
shared crate (`core`, `protocol`) or in the root `Cargo.toml` goes back to the
coordinator. A piece is done when `just ci` passes; the coordinator merges a
wave once every piece in it is green, then the next wave starts.
