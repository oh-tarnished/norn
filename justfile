# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Norn's local entry points. GitHub workflows run these same commands.

python := if os() == "windows" { "python" } else { "python3" }

# Compile every workspace binary and library without changing Cargo.lock.
build:
    cargo build --workspace --locked

# The repository gates: size, purity, docs, arch, deps.
gates:
    cargo run --locked -p xtask -- gates

# Unit, integration and documentation tests, including the gate tests.
test:
    cargo test --workspace --locked --no-fail-fast

# The real nornctl against a real daemon, the bugs unit tests cannot see.
e2e:
    cargo test --locked -p norn-cli --tests

# Killed processes and damaged files: recovery must keep what was acknowledged.
crash:
    cargo test --locked -p norn-storage --test crash --test compaction_crash --test corruption
    cargo test --locked -p norn-codec --test corruption
    cargo test --locked -p norn-runtime --test journal
    cargo test --locked -p norn-metadata --test batch_recovery
    cargo test --locked -p norn-server --test crash --test compaction_kill --test shared_memory_crash --test query_damage

# Benchmark smoke runs include real daemons over Unix sockets.
[unix]
bench-smoke:
    cargo test --workspace --benches --locked

# Compile benchmarks on Windows; the daemon benchmarks require Unix sockets.
[windows]
bench-smoke:
    cargo check --workspace --benches --locked

# The benchmark matrix: --quick (minutes) or --full (hours); docs/performance.md.
bench-matrix *args:
    cargo bench --locked -p norn-server --bench matrix -- {{ args }}

# Formatting and lints, as CI runs them.
lint:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets --locked -- -D warnings

# Rustdoc with warnings as errors.
doc:
    RUSTDOCFLAGS=-Dwarnings cargo doc --no-deps --workspace --locked

# Licences, advisories and sources across the whole graph.
deny:
    cargo deny --locked check

# Protobuf style and formatting. Missing schemas must fail the gate.
buf:
    buf lint
    buf format --diff --exit-code

# Compare the wire/JSON contract with a Git or Buf image baseline.
buf-breaking against:
    buf breaking --against "{{ against }}"

# Google AIP lint over protobuf/, through a buf image so googleapis imports
# resolve. Run from protobuf/ so each file's path matches its package. Needs
# api-linter 2.3.1 (go install github.com/googleapis/api-linter/v2/cmd/api-linter@v2.3.1).
api-lint:
    #!/usr/bin/env bash
    set -euo pipefail
    image="$(mktemp -t norn-image.XXXXXX)"
    trap 'rm -f "$image"' EXIT
    buf build -o "$image"
    cd protobuf && api-linter --set-exit-status --ignore-comment-disables \
        --descriptor-set-in "$image" $(find norn -name '*.proto' | sort)

# Install the same pinned AIP linter used by CI (requires Go).
api-lint-tools:
    go install github.com/googleapis/api-linter/v2/cmd/api-linter@v2.3.1

# Regenerate crates/protocol/src/generated, and the daemon's MCP handlers in
# crates/server/src/mcp/generated, from protobuf/. The output is committed, so
# building needs neither buf nor protoc. Needs the plugins buf.gen.yaml and
# buf.gen.mcp.yaml name (`just gen-tools`).
gen:
    buf generate
    buf generate --template buf.gen.mcp.yaml
    bash tools/license-mcp.sh
    buf build --exclude-source-info -o crates/protocol/src/norn.binpb
    buf build --path protobuf/norn/type/sample.proto --as-file-descriptor-set \
        --exclude-source-info -o adapters/mcap/src/generated/norn_type_sample.binpb

# Install the local code generators declared by buf.gen.yaml and
# buf.gen.mcp.yaml (protoc-gen-mcp is Go: requires Go, its bin on PATH).
gen-tools:
    cargo install --locked protoc-gen-prost@0.5.0 protoc-gen-tonic@0.5.0 protoc-gen-prost-crate@0.5.0
    go install github.com/the-protobuf-project/mcp/plugin/cmd/protoc-gen-mcp@v1.7.8

# Fails when the committed generated code is not what protobuf/ produces: a
# .proto edited without `just gen` ships a crate that disagrees with it.
gen-check:
    #!/usr/bin/env bash
    set -euo pipefail
    generated="$(mktemp -d -t norn-generated.XXXXXX)"
    trap 'rm -rf "$generated"' EXIT
    buf generate --output "$generated"
    diff -ru crates/protocol/src/generated "$generated/crates/protocol/src/generated"
    buf generate --template buf.gen.mcp.yaml --output "$generated"
    bash tools/license-mcp.sh "$generated/crates/server/src/mcp/generated"
    diff -ru crates/server/src/mcp/generated "$generated/crates/server/src/mcp/generated"
    buf build --exclude-source-info -o "$generated/norn.binpb"
    cmp crates/protocol/src/norn.binpb "$generated/norn.binpb"
    buf build --path protobuf/norn/type/sample.proto --as-file-descriptor-set \
        --exclude-source-info -o "$generated/sample.binpb"
    cmp adapters/mcap/src/generated/norn_type_sample.binpb "$generated/sample.binpb"

# Fuzz every target, or just TARGET, for SECONDS each. Needs nightly and
# cargo-fuzz (cargo install cargo-fuzz --locked). The corpus is regenerated from
# Norn's own writers each run; a crash lands in fuzz/artifacts/TARGET/. Replay one
# with `cargo +nightly fuzz run TARGET fuzz/artifacts/TARGET/crash-...`.
fuzz seconds='60' target='':
    #!/usr/bin/env bash
    set -euo pipefail
    cargo +nightly run --quiet --manifest-path fuzz/Cargo.toml -p norn-fuzz-seed -- fuzz/corpus
    targets="{{ target }}"
    if [ -z "$targets" ]; then
        targets="$(cargo +nightly fuzz list)"
    fi
    for target in $targets; do
        echo "fuzz $target for {{ seconds }} s"
        cargo +nightly fuzz run "$target" -- -max_total_time={{ seconds }} \
            -max_len=1048576 -len_control=100 -timeout=30 -rss_limit_mb=6144 -malloc_limit_mb=2048
    done

# Regenerates docs/images from a real daemon; takes a little over a minute.
screenshots:
    cargo build --locked -p norn-server -p norn-cli
    tools/screenshots/capture.sh target/debug docs/images

# Check the whole workspace for another Rust target, without a C compiler.
cross target:
    cargo check --workspace --all-targets --locked --target "{{ target }}"

# Validate the release tag/version and write GitHub's build matrix.
release-plan:
    bash .github/release/plan.sh

# Build and package the daemon and CLI for an installed Rust target.
release-build target:
    cargo build --release --locked -p norn-server -p norn-cli --target "{{ target }}"

release-package target version:
    {{ python }} .github/release/package.py "{{ target }}" "{{ version }}"

# All local checks. PR CI also compares protobuf against its base commit
# with `just buf-breaking ".git#ref=<base-sha>"`.
ci: build lint test bench-smoke doc gates deny buf api-lint gen-check
