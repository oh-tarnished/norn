#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Release metadata must yield complete, authenticated Homebrew downloads.
set -euo pipefail
scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
fixture="$(mktemp -d -t norn-formula.XXXXXX)"
trap 'rm -rf "$fixture"' EXIT
sum="$(printf '%064d' 0)"
targets=(aarch64-apple-darwin x86_64-apple-darwin aarch64-unknown-linux-gnu x86_64-unknown-linux-gnu)
for target in "${targets[@]}"; do
  name="norn-1.0.0-rc1-$target.tar.gz"
  printf '%s  %s\n' "$sum" "$name" > "$fixture/$name.sha256"
done
jq -n --arg sum "$sum" --args '
  {tag_name: "v1.0.0-rc1", assets: ($ARGS.positional | to_entries | map({
    id: (.key + 1), name: ("norn-1.0.0-rc1-" + .value + ".tar.gz"), digest: ("sha256:" + $sum)
  }))}' "${targets[@]}" > "$fixture/original.json"
cp "$fixture/original.json" "$fixture/release.json"
GITHUB_REPOSITORY=shokki-engineering/norn bash "$scripts/formula.sh" 1.0.0-rc1 "$fixture" v1.0.0-rc1 > "$fixture/norn.rb"
[[ "$(grep -c 'headers: download_headers' "$fixture/norn.rb")" == 4 ]]
grep -q 'ENV\["HOMEBREW_GITHUB_API_TOKEN"\]' "$fixture/norn.rb"
grep -q 'Accept: application/octet-stream' "$fixture/norn.rb"
grep -q 'https://api.github.com/repos/shokki-engineering/norn/releases/assets/4' "$fixture/norn.rb"
! grep -q '@VERSION@\|releases/download/' "$fixture/norn.rb"
for mutation in 'del(.assets[0])' '.assets += [.assets[0]]' \
  '.assets[0].digest = "sha256:incorrect"' '.tag_name = "v1.0.0-rc2"'; do
  jq "$mutation" "$fixture/original.json" > "$fixture/release.json"
  if bash "$scripts/formula.sh" 1.0.0-rc1 "$fixture" v1.0.0-rc1 > "$fixture/out" 2> "$fixture/error"; then
    echo "invalid release metadata was accepted: $mutation" >&2; exit 1
  fi
  [[ ! -s "$fixture/out" ]]
done
if [ -n "${FORMULA_OUTPUT:-}" ]; then
  cp "$fixture/norn.rb" "$FORMULA_OUTPUT"
fi
echo "formula: private downloads and four invalid-release cases passed"
