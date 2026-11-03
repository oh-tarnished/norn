#!/usr/bin/env bash
# Render the Ruby formula after uploading archives, using bacnet-rs a534df3's
# authenticated asset API URLs and the release's checksum files.
set -euo pipefail
version="$1"
directory="$2"
tag="$3"
repository="${GITHUB_REPOSITORY:-shokki-engineering/norn}"
scripts="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]] || {
  echo "invalid release version" >&2; exit 1;
}
[[ "$tag" == "v$version" ]] || { echo "tag does not match version" >&2; exit 1; }
[[ "$repository" =~ ^[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+$ ]] || {
  echo "expected repository owner/name" >&2; exit 1;
}
jq -e --arg tag "$tag" '.tag_name == $tag' "$directory/release.json" > /dev/null

asset() {
  local filename sum named extra id digest
  filename="norn-${version}-$1.tar.gz"
  read -r sum named extra < "$directory/$filename.sha256"
  [[ "$sum" =~ ^[a-fA-F0-9]{64}$ && "${named#\*}" == "$filename" && -z "$extra" ]] || {
    echo "invalid checksum or archive name: $filename" >&2; return 1;
  }
  id="$(jq -er --arg name "$filename" '
    [.assets[] | select(.name == $name)] |
    if length == 1 then .[0].id else error("missing or duplicate asset: " + $name) end |
    if type == "number" and . > 0 and . == floor then . else error("invalid asset ID") end
  ' "$directory/release.json")" || return 1
  digest="$(jq -r --arg name "$filename" '.assets[] | select(.name == $name) | .digest // ""' \
    "$directory/release.json")"
  [[ -z "$digest" || "$digest" == "sha256:$sum" ]] || {
    echo "uploaded asset digest disagrees: $filename" >&2; return 1;
  }
  printf '      url "https://api.github.com/repos/%s/releases/assets/%s",\n          using: :curl,\n          headers: download_headers\n      sha256 "%s"' \
    "$repository" "$id" "$sum"
}

# Assignments fail before anything is emitted: never publish partial Ruby.
mac_arm="$(asset aarch64-apple-darwin)"
mac_intel="$(asset x86_64-apple-darwin)"
linux_arm="$(asset aarch64-unknown-linux-gnu)"
linux_intel="$(asset x86_64-unknown-linux-gnu)"
formula="$(cat "$scripts/norn.rb.in")"
formula="${formula//@VERSION@/$version}"
formula="${formula//@REPOSITORY@/$repository}"
formula="${formula//@MAC_ARM@/$mac_arm}"
formula="${formula//@MAC_INTEL@/$mac_intel}"
formula="${formula//@LINUX_ARM@/$linux_arm}"
formula="${formula//@LINUX_INTEL@/$linux_intel}"
printf '%s\n' "$formula"
