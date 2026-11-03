#!/usr/bin/env python3
"""Render a private-release Homebrew formula using bacnet's asset API strategy."""

import argparse
import json
import os
import re
from pathlib import Path


TARGETS = {
    "MAC_ARM": "aarch64-apple-darwin",
    "MAC_INTEL": "x86_64-apple-darwin",
    "LINUX_ARM": "aarch64-unknown-linux-gnu",
    "LINUX_INTEL": "x86_64-unknown-linux-gnu",
}


def render(version, directory, tag, repository):
    """Require one uploaded archive and a valid SHA-256 for every platform."""
    if not re.fullmatch(r"[0-9]+\.[0-9]+\.[0-9]+(?:-[0-9A-Za-z.-]+)?", version):
        raise ValueError("invalid release version")
    if tag != f"v{version}":
        raise ValueError("tag does not match version")
    if not re.fullmatch(r"[A-Za-z0-9_.-]+/[A-Za-z0-9_.-]+", repository):
        raise ValueError("expected repository owner/name")
    release = json.loads((directory / "release.json").read_text())
    if release["tag_name"] != tag:
        raise ValueError("release.json does not match the requested tag")
    replacements = {"VERSION": version, "REPOSITORY": repository}
    for key, target in TARGETS.items():
        filename = f"norn-{version}-{target}.tar.gz"
        matches = [asset for asset in release["assets"] if asset["name"] == filename]
        if len(matches) != 1:
            raise ValueError(f"missing or duplicate asset: {filename}")
        asset_id = matches[0]["id"]
        if type(asset_id) is not int or asset_id <= 0:
            raise ValueError(f"invalid asset ID: {filename}")
        fields = (directory / f"{filename}.sha256").read_text().split()
        if len(fields) != 2 or fields[1].lstrip("*") != filename:
            raise ValueError(f"checksum must name its archive: {filename}")
        digest = fields[0]
        if not re.fullmatch(r"[a-fA-F0-9]{64}", digest):
            raise ValueError(f"invalid SHA-256: {filename}")
        uploaded_digest = matches[0].get("digest")
        if uploaded_digest and uploaded_digest != f"sha256:{digest.lower()}":
            raise ValueError(f"uploaded asset digest disagrees: {filename}")
        replacements[key] = (
            f'      url "https://api.github.com/repos/{repository}/releases/assets/{asset_id}",\n'
            '          using: :curl,\n'
            '          headers: download_headers\n'
            f'      sha256 "{digest.lower()}"'
        )
    template = Path(__file__).with_name("norn.rb.in").read_text()
    for key, value in replacements.items():
        template = template.replace(f"@{key}@", value)
    return template


def main():
    """Emit only a complete formula; validation failures cannot publish partial Ruby."""
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("version")
    parser.add_argument("directory", type=Path)
    parser.add_argument("tag")
    parser.add_argument("--repository", default=os.environ.get(
        "GITHUB_REPOSITORY", "shokki-engineering/norn"))
    args = parser.parse_args()
    print(render(args.version, args.directory, args.tag, args.repository), end="")


if __name__ == "__main__":
    main()
