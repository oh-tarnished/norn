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

# Benchmark smoke runs include real daemons over Unix sockets.
[unix]
bench-smoke:
    cargo test --workspace --benches --locked

# Compile benchmarks on Windows; the daemon benchmarks require Unix sockets.
[windows]
bench-smoke:
    cargo check --workspace --benches --locked

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

# Regenerate crates/protocol/src/generated from protobuf/. The output is
# committed, so building needs neither buf nor protoc. Needs the plugins
# buf.gen.yaml names (cargo install protoc-gen-prost@0.5.0
# protoc-gen-tonic@0.5.0 protoc-gen-prost-crate@0.5.0).
gen:
    buf generate

# Install the local code generators declared by buf.gen.yaml.
gen-tools:
    cargo install --locked protoc-gen-prost@0.5.0 protoc-gen-tonic@0.5.0 protoc-gen-prost-crate@0.5.0

# Fails when the committed generated code is not what protobuf/ produces: a
# .proto edited without `just gen` ships a crate that disagrees with it.
gen-check:
    #!/usr/bin/env bash
    set -euo pipefail
    generated="$(mktemp -d -t norn-generated.XXXXXX)"
    trap 'rm -rf "$generated"' EXIT
    buf generate --output "$generated"
    diff -ru crates/protocol/src/generated "$generated/crates/protocol/src/generated"

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
