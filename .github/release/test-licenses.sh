#!/usr/bin/env bash
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

# Check the root license files shipped in every archive and Homebrew installation.
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
  require_text "$directory/THIRD-PARTY-NOTICES.md" "Apache License" || return 1
  require_text "$directory/THIRD-PARTY-NOTICES.md" "Version 2.0, January 2004" || return 1
  require_text "$directory/THIRD-PARTY-NOTICES.md" "END OF TERMS AND CONDITIONS" || return 1
}

check "$root"
if [[ "${1:-}" == "--self-test" ]]; then
  fixture="$(mktemp -d -t norn-licenses.XXXXXX)"
  trap 'rm -rf "$fixture"' EXIT
  documents=(LICENSE LICENSE-COMMERCIAL.md THIRD-PARTY-NOTICES.md)
  for document in "${documents[@]}"; do
    cp "$root/$document" "$fixture/$document"
  done
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
  for document in LICENSE THIRD-PARTY-NOTICES.md; do
    cp "$fixture/$document" "$fixture/$document.saved"
    echo "This is not the expected license text." > "$fixture/$document"
    if check "$fixture" > "$fixture/output" 2>&1; then
      echo "license check accepted invalid text in $document" >&2; exit 1
    fi
    mv "$fixture/$document.saved" "$fixture/$document"
  done
  echo "licenses: root files and eight missing, empty or invalid-file cases passed"
else
  echo "licenses: root project, commercial and third-party files passed"
fi
