"""Release metadata must yield complete, authenticated Homebrew downloads."""

import json
import tempfile
import unittest
from pathlib import Path

from formula import TARGETS, render


class FormulaTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="norn-formula-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.assets = []
        for asset_id, target in enumerate(TARGETS.values(), 1):
            name = f"norn-0.1.0-{target}.tar.gz"
            self.assets.append({"id": asset_id, "name": name, "digest": "sha256:" + "a" * 64})
            (self.directory / f"{name}.sha256").write_text("a" * 64 + f"  {name}\n")
        self.save()

    def save(self, tag="v0.1.0"):
        (self.directory / "release.json").write_text(json.dumps({
            "tag_name": tag, "assets": self.assets,
        }))

    def render(self):
        return render("0.1.0", self.directory, "v0.1.0", "shokki-engineering/norn")

    def test_every_platform_uses_the_existing_private_asset_strategy(self):
        formula = self.render()
        self.assertEqual(formula.count('using: :curl'), 4)
        self.assertEqual(formula.count('headers: download_headers'), 4)
        self.assertIn('ENV["HOMEBREW_GITHUB_API_TOKEN"]', formula)
        self.assertIn('Accept: application/octet-stream', formula)
        self.assertIn('releases/assets/4', formula)
        self.assertNotIn('@VERSION@', formula)
        self.assertNotIn('releases/download/', formula)

    def test_an_unuploaded_archive_is_refused(self):
        self.assets.pop()
        self.save()
        with self.assertRaisesRegex(ValueError, "missing or duplicate"):
            self.render()

    def test_duplicate_assets_are_refused(self):
        self.assets.append(self.assets[0])
        self.save()
        with self.assertRaisesRegex(ValueError, "missing or duplicate"):
            self.render()

    def test_the_checksum_must_match_the_uploaded_archive(self):
        self.assets[0]["digest"] = "sha256:" + "b" * 64
        self.save()
        with self.assertRaisesRegex(ValueError, "digest disagrees"):
            self.render()

    def test_the_release_tag_must_match_the_version(self):
        self.save("v0.2.0")
        with self.assertRaisesRegex(ValueError, "requested tag"):
            self.render()


if __name__ == "__main__":
    unittest.main()
