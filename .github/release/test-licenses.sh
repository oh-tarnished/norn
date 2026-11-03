#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Check the root license files and Norn's generated MCP license headers.
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"

require_text() {
  local path="$1" text="$2"
  if [[ ! -s "$path" ]] || ! grep -Fq "$text" "$path"; then
    echo "license check: $path is missing, empty or does not contain $text" >&2
    return 1
  fi
}

check() {
  local directory="$1"
  require_text "$directory/LICENSE" "GNU AFFERO GENERAL PUBLIC LICENSE" || return 1
  require_text "$directory/LICENSE" "Version 3, 19 November 2007" || return 1
  require_text "$directory/LICENSE" "END OF TERMS AND CONDITIONS" || return 1
  require_text "$directory/LICENSE-COMMERCIAL.md" "# Commercial licence" || return 1
  local file count=0
  while IFS= read -r -d '' file; do
    require_text "$file" "// SPDX-FileCopyrightText: 2026 Srikanth K" || return 1
    require_text "$file" "// SPDX-License-Identifier: AGPL-3.0-or-later" || return 1
    count=$((count + 1))
  done < <(find "$directory/crates/server/src/mcp/generated" -type f -name '*.mcp.rs' -print0)
  if [[ "$count" == 0 ]]; then
    echo "license check: no generated MCP handlers examined" >&2; return 1
  fi
}

check "$root"
if [[ "${1:-}" == "--self-test" ]]; then
  fixture="$(mktemp -d -t norn-licenses.XXXXXX)"
  trap 'rm -rf "$fixture"' EXIT
  documents=(LICENSE LICENSE-COMMERCIAL.md)
  for document in "${documents[@]}"; do
    cp "$root/$document" "$fixture/$document"
  done
  generated="$fixture/crates/server/src/mcp/generated"
  mkdir -p "$generated"
  cp "$root/crates/server/src/mcp/generated/norn/admin/v1/admin_service.mcp.rs" "$generated/test.mcp.rs"
  check "$fixture"
  for document in "${documents[@]}"; do
    mv "$fixture/$document" "$fixture/$document.saved"
    if check "$fixture" > "$fixture/output" 2>&1; then
      echo "license check accepted a missing $document" >&2; exit 1
    fi
    touch "$fixture/$document"
    if check "$fixture" > "$fixture/output" 2>&1; then
      echo "license check accepted an empty $document" >&2; exit 1
    fi
    mv "$fixture/$document.saved" "$fixture/$document"
  done
  for document in LICENSE; do
    cp "$fixture/$document" "$fixture/$document.saved"
    echo "This is not the expected license text." > "$fixture/$document"
    if check "$fixture" > "$fixture/output" 2>&1; then
      echo "license check accepted invalid text in $document" >&2; exit 1
    fi
    mv "$fixture/$document.saved" "$fixture/$document"
  done
  cp "$generated/test.mcp.rs" "$fixture/handler.saved"
  sed 's/AGPL-3.0-or-later/Apache-2.0/' "$fixture/handler.saved" > "$generated/test.mcp.rs"
  if check "$fixture" > "$fixture/output" 2>&1; then
    echo "license check accepted a mismatched generated license" >&2; exit 1
  fi
  : > "$generated/test.mcp.rs"
  if check "$fixture" > "$fixture/output" 2>&1; then
    echo "license check accepted an empty generated handler" >&2; exit 1
  fi
  mv "$generated/test.mcp.rs" "$fixture/empty-handler.saved"
  if check "$fixture" > "$fixture/output" 2>&1; then
    echo "license check accepted no generated handlers" >&2; exit 1
  fi
  echo "licenses: root files, generated headers and eight negative cases passed"
else
  echo "licenses: root project, commercial files and generated MCP headers passed"
fi
