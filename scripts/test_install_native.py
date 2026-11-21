#!/usr/bin/env python3
"""Test of install_native.sh, the install step of a build from source (documentation/building-from-source.md): right files in the right places below DESTDIR, both
programs in ONE folder (the window finds tsw-launch next to itself), nothing outside DESTDIR, a clear error when something is missing."""
import os
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, ".."))
SCRIPT = os.path.join(HERE, "install_native.sh")
ID = "org.tswlauncher.Launcher"


def run(args):
    return subprocess.run(["sh", SCRIPT, *args], capture_output=True, text=True)


class Install(unittest.TestCase):
    def setUp(self):
        self.t = tempfile.mkdtemp(prefix="tsw-install-")
        self.bin = os.path.join(self.t, "dist")
        os.makedirs(self.bin)
        for n in ("tsw-launch", "tsw-launcher"):
            open(os.path.join(self.bin, n), "wb").write(b"\x7fELF placeholder")
        self.dest = os.path.join(self.t, "stage")

    def files(self):
        out = set()
        for d, _, fs in os.walk(self.dest):
            for f in fs:
                out.add(os.path.relpath(os.path.join(d, f), self.dest))
        return out

    def test_it_installs_both_programs_into_one_folder_with_the_desktop_data_and_the_licences(self):
        p = run([ROOT, self.bin, self.dest, "--prefix", "/usr/local"])
        self.assertEqual(p.returncode, 0, p.stderr)
        files = self.files()
        for rel in ("usr/local/bin/tsw-launch", "usr/local/bin/tsw-launcher", f"usr/local/share/applications/{ID}.desktop", f"usr/local/share/metainfo/{ID}.metainfo.xml",
                    f"usr/local/share/icons/hicolor/scalable/apps/{ID}.svg", f"usr/local/share/icons/hicolor/256x256/apps/{ID}.png",
                    "usr/local/share/licenses/tsw-forever/LICENSES/inventory.json", "usr/local/share/licenses/tsw-forever/LICENSES/README.md", "usr/local/share/licenses/tsw-forever/LICENSE", "usr/local/share/licenses/tsw-forever/THIRD-PARTY-NOTICES.md", "usr/local/share/doc/tsw-forever/README.md"):
            self.assertIn(rel, files)
        repeated = run([ROOT, self.bin, self.dest, "--prefix", "/usr/local"])
        self.assertEqual(repeated.returncode, 0, repeated.stderr)
        self.assertFalse(any('/LICENSES/LICENSES/' in f for f in self.files()), "reinstall must not nest licence directories")
        self.assertTrue(os.access(os.path.join(self.dest, "usr/local/bin/tsw-launcher"), os.X_OK))
        self.assertFalse(os.access(os.path.join(self.dest, f"usr/local/share/applications/{ID}.desktop"), os.X_OK))

    def test_nothing_is_written_outside_destdir(self):
        before = set(os.listdir(self.t))
        run([ROOT, self.bin, self.dest])
        self.assertEqual(set(os.listdir(self.t)) - before, {"stage"})
        self.assertTrue(all(f.startswith("usr/") for f in self.files()))

    def test_the_default_prefix_is_usr_and_the_licence_dir_can_be_chosen(self):
        run([ROOT, self.bin, self.dest, "--licence-dir", "/opt/lic"])
        self.assertIn("usr/bin/tsw-launcher", self.files())
        self.assertIn("opt/lic/LICENSE", self.files())

    def test_a_missing_program_or_source_file_is_a_clear_error_and_installs_nothing(self):
        os.remove(os.path.join(self.bin, "tsw-launch"))
        p = run([ROOT, self.bin, self.dest])
        self.assertEqual(p.returncode, 1)
        self.assertIn("missing", p.stderr)
        self.assertFalse(os.path.exists(self.dest))

    def test_wrong_usage_is_refused(self):
        self.assertEqual(run([]).returncode, 2)
        self.assertEqual(run([ROOT, self.bin, self.dest, "--bogus"]).returncode, 2)


if __name__ == "__main__":
    unittest.main()
