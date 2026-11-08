#!/usr/bin/env python3
"""Source staging must discard excluded leftovers, including on reused build trees."""
from pathlib import Path
import shutil
import subprocess
import tarfile
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().with_name("native_stage.sh")


class SourceStage(unittest.TestCase):
    def test_reused_staging_removes_excluded_files_and_matches_a_clean_archive(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            (root / "scripts").mkdir()
            shutil.copy2(SCRIPT, root / "scripts/native_stage.sh")
            (root / "scripts/version.py").write_text('print("0.2.0-rc.1")\n')
            (root / "README.md").write_text("fixture source\n")

            def stage():
                subprocess.run(["bash", str(root / "scripts/native_stage.sh")], check=True, capture_output=True)
                return (root / "build/native/tsw-forever-0.2.0-rc.1-src.tar.gz").read_bytes()

            clean = stage()
            stage_root = root / "build/native/stage/tsw-forever-0.2.0-rc.1"
            obsolete = stage_root / "packaging/obsolete/recipe.sh"
            obsolete.parent.mkdir(parents=True)
            obsolete.write_text("excluded fixture\n")
            self.assertEqual(stage(), clean)
            self.assertFalse(obsolete.exists())
            with tarfile.open(root / "build/native/tsw-forever-0.2.0-rc.1-src.tar.gz") as archive:
                self.assertNotIn("tsw-forever-0.2.0-rc.1/packaging/obsolete/recipe.sh", archive.getnames())


if __name__ == "__main__":
    unittest.main()
