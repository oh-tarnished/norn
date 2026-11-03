#!/usr/bin/env python3
# SPDX-FileCopyrightText: 2026 Srikanth K
# SPDX-License-Identifier: AGPL-3.0-or-later

"""Package native Norn binaries, documentation and their SHA-256 checksum."""

import hashlib
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path


def main():
    """Smoke-check the binaries and create one platform archive in dist/."""
    target, version = sys.argv[1:]
    suffix = ".exe" if "windows" in target else ""
    binaries = Path("target") / target / "release"
    subprocess.run([str(binaries / f"norn{suffix}"), "--version"], check=True)
    subprocess.run([str(binaries / f"nornctl{suffix}"), "--help"], check=True)
    name = f"norn-{version}-{target}"
    dist = Path("dist")
    dist.mkdir(exist_ok=True)
    with tempfile.TemporaryDirectory(prefix="norn-release-") as temporary:
        package = Path(temporary) / name
        package.mkdir()
        for binary in ("norn", "nornctl"):
            shutil.copy2(binaries / f"{binary}{suffix}", package)
        for document in (
            "LICENSE", "LICENSE-COMMERCIAL.md", "README.md",
            "CONTRIBUTING.md", "SECURITY.md",
            "REUSE.toml",
        ):
            shutil.copy2(document, package)
        shutil.copytree("docs", package / "docs")
        shutil.copytree("LICENSES", package / "LICENSES")
        archive = Path(shutil.make_archive(
            str((dist / name).resolve()), "zip" if suffix else "gztar",
            root_dir=temporary, base_dir=name,
        ))
    digest = hashlib.sha256()
    with archive.open("rb") as source:
        for chunk in iter(lambda: source.read(1024 * 1024), b""):
            digest.update(chunk)
    archive.with_name(archive.name + ".sha256").write_text(
        f"{digest.hexdigest()}  {archive.name}\n", encoding="utf-8",
    )
    print(f"Packaged {archive}")


if __name__ == "__main__":
    main()
