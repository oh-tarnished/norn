#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Validate every workspace package's version, then emit the release matrix.
# Local preview: GITHUB_EVENT_NAME=push GITHUB_REF_NAME=v1.0.0-rc.2 \
#   GITHUB_OUTPUT=/tmp/norn-release-plan just release-plan
set -euo pipefail

metadata="$(cargo metadata --locked --no-deps --format-version 1)"
version="$(printf '%s\n' "$metadata" | python3 -c '
import json, sys
metadata = json.load(sys.stdin)
members = set(metadata["workspace_members"])
versions = {p["version"] for p in metadata["packages"] if p["id"] in members}
if len(versions) != 1:
    sys.exit("workspace package versions disagree: " + ", ".join(sorted(versions)))
print(versions.pop())
')"
if [ "${GITHUB_EVENT_NAME:-}" = workflow_dispatch ]; then
  tag="v$version"
elif [ "${GITHUB_EVENT_NAME:-}" = push ]; then
  tag="${GITHUB_REF_NAME:?GITHUB_REF_NAME must name the release tag}"
else
  echo "::error::expected a tag push or workflow_dispatch"
  exit 1
fi
if [ "$tag" != "v$version" ]; then
  echo "::error::tag $tag disagrees with workspace version v$version"
  exit 1
fi
matrix="$(python3 -c '
import json
platforms = {
    "x86_64-unknown-linux-gnu": "ubuntu-22.04",
    "aarch64-unknown-linux-gnu": "ubuntu-22.04-arm",
    "aarch64-apple-darwin": "macos-15",
    "x86_64-apple-darwin": "macos-15-intel",
    "x86_64-pc-windows-msvc": "windows-latest",
}
print(json.dumps({"include": [{"target": t, "os": o} for t, o in platforms.items()]}))
')"
{
  echo "version=$version"
  echo "tag=$tag"
  echo "title=norn $tag"
  echo "matrix=$matrix"
} >> "${GITHUB_OUTPUT:?GITHUB_OUTPUT must name an output file}"
echo "Releasing norn and nornctl as $tag"
