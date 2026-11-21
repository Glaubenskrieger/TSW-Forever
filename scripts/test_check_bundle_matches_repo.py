#!/usr/bin/env python3
"""scripts/check_bundle_matches_repo.sh with throw-away keys: only the bundle of the signed repository, built with the update source and the public key, passes."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from testtools import requires_tools  # noqa: E402
KEYS = os.path.join(HERE, "flatpak_keys.sh")
CHECK = os.path.join(HERE, "check_bundle_matches_repo.sh")
URL = "https://example.invalid/TSW-Forever/repo"
APP = "org.tswtest.Bundle"


def sh(script, *args):
    return subprocess.run(["bash", script, *args], capture_output=True, text=True)


def export(d, name, repo, marker):
    app = os.path.join(d, name)
    os.makedirs(os.path.join(app, "files"))
    os.makedirs(os.path.join(app, "export"))
    with open(os.path.join(app, "files", "marker"), "w") as f:
        f.write(marker)
    with open(os.path.join(app, "metadata"), "w") as f:
        f.write(f"[Application]\nname={APP}\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
    r = subprocess.run(["flatpak", "build-export", repo, app, "stable"], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr


def bundle(repo, out, *extra):
    r = subprocess.run(["flatpak", "build-bundle", *extra, repo, out, APP, "stable"], capture_output=True, text=True)
    assert r.returncode == 0, r.stderr


@requires_tools("flatpak", "gpg", "ostree", "python3")
class BundleMatches(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-bundle-")
        home = os.path.join(cls.d, "gnupg")
        fpr = sh(KEYS, "generate-split", home, "TSW bundle throw-away <b@example.invalid>").stdout.strip().splitlines()[-1]
        cls.pub = os.path.join(cls.d, "pub.gpg")
        assert sh(KEYS, "export-public", home, fpr, cls.pub).returncode == 0
        other = os.path.join(cls.d, "gnupg2")
        fpr2 = sh(KEYS, "generate-split", other, "TSW other throw-away <o@example.invalid>").stdout.strip().splitlines()[-1]
        cls.pub2 = os.path.join(cls.d, "pub2.gpg")
        assert sh(KEYS, "export-public", other, fpr2, cls.pub2).returncode == 0
        cls.repo = os.path.join(cls.d, "repo")
        export(cls.d, "app1", cls.repo, "one")
        assert sh(KEYS, "sign-repo", home, fpr, cls.repo).returncode == 0
        cls.good = os.path.join(cls.d, "good.flatpak")
        bundle(cls.repo, cls.good, f"--repo-url={URL}", f"--gpg-keys={cls.pub}")
        cls.nokey = os.path.join(cls.d, "nokey.flatpak")
        bundle(cls.repo, cls.nokey)
        # a bundle of ANOTHER build (an unsigned CI test build) with the same ref
        cls.other_repo = os.path.join(cls.d, "other-repo")
        export(cls.d, "app2", cls.other_repo, "two")
        cls.local = os.path.join(cls.d, "local.flatpak")
        bundle(cls.other_repo, cls.local, f"--repo-url={URL}", f"--gpg-keys={cls.pub}")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.d, ignore_errors=True)

    def test_the_bundle_of_the_signed_repository_passes(self):
        r = sh(CHECK, self.good, self.repo, self.pub, URL)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("bundle matches", r.stdout)

    def test_a_bundle_of_another_build_is_refused_although_it_has_the_update_source(self):
        r = sh(CHECK, self.local, self.repo, self.pub, URL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("not the bundle of the signed repository", r.stderr)

    def test_a_bundle_without_the_update_source_or_with_another_key_or_address_is_refused(self):
        r = sh(CHECK, self.nokey, self.repo, self.pub, URL)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not carry", r.stderr)
        self.assertEqual(sh(CHECK, self.good, self.repo, self.pub2, URL).returncode, 1, "another public key")
        self.assertEqual(sh(CHECK, self.good, self.repo, self.pub, "https://other.invalid/repo").returncode, 1, "another address")

    def test_bad_usage_is_refused(self):
        self.assertEqual(sh(CHECK, self.good, self.repo, self.pub, "http://example.invalid/repo").returncode, 2)
        self.assertEqual(sh(CHECK, os.path.join(self.d, "missing.flatpak"), self.repo, self.pub, URL).returncode, 2)
        self.assertEqual(sh(CHECK, self.good, os.path.join(self.d, "no-repo"), self.pub, URL).returncode, 2)
        junk = os.path.join(self.d, "junk.flatpak")
        with open(junk, "wb") as f:
            f.write(b"not a bundle")
        self.assertEqual(sh(CHECK, junk, self.repo, self.pub, URL).returncode, 1)


if __name__ == "__main__":
    unittest.main()
