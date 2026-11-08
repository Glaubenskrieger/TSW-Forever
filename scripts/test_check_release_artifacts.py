#!/usr/bin/env python3
"""Tests of scripts/check_release_artifacts.py: a clean tree passes; every kind of forbidden content is found; the real release set (if present) is clean."""
import io
import os
import sys
import tarfile
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import check_release_artifacts as c  # noqa: E402


def make_tar(path, files, mode="w:gz"):
    with tarfile.open(path, mode) as t:
        for name, data in files.items():
            ti = tarfile.TarInfo(name)
            ti.size = len(data)
            t.addfile(ti, io.BytesIO(data))


class Audit(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp()

    def tgz(self, files):
        p = os.path.join(self.d, "a.tar.gz")
        make_tar(p, files)
        return p

    def test_a_clean_artifact_passes(self):
        ok = {"usr/bin/tsw-launch": b"\x7fELF" + b"\0" * 8, "usr/share/doc/README.md": b"# readme", "usr/share/icons/a.png": b"\x89PNG"}
        self.assertEqual(c.scan(self.tgz(ok)), [])

    def test_microsoft_and_funcom_files_are_found_by_name(self):
        for name in ["x/d3dcompiler_47.dll", "x/d3dx9_43.dll", "x/TheSecretWorldDX11.exe", "x/ClientPatcher.exe", "x/RDB/00.rdbdata", "x/redist.cab",
                     "x/prefix/drive_c/windows/system32/a", "x/system.reg", "x/Funcom/readme", "x/id_ed25519", "x/key.pem", "x/dxvk.conf"]:
            self.assertTrue(c.scan(self.tgz({name: b"data"})), name)

    def test_a_windows_program_is_found_by_its_bytes_whatever_its_name(self):
        found = c.scan(self.tgz({"share/innocent.bin": b"MZ\x90\x00" + b"\0" * 60}))
        self.assertTrue(any("MZ header" in f for f in found), found)

    def test_an_oversized_file_is_found(self):
        p = os.path.join(self.d, "big")
        os.mkdir(p)
        with open(os.path.join(p, "x.bin"), "wb") as f:
            f.truncate(c.MAX_SIZE + 1)
        self.assertTrue(any("limit" in b for b in c.scan(p)))

    def test_the_main_function_reports_and_sets_the_exit_code(self):
        self.assertEqual(c.main([self.tgz({"a": b"x"})]), 0)
        self.assertEqual(c.main([self.tgz({"a/foo.dll": b"x"})]), 1)
        self.assertEqual(c.main([os.path.join(self.d, "missing.tar.gz")]), 2)
        self.assertEqual(c.main([]), 2)

    def test_only_what_is_published_is_audited_other_package_formats_are_refused(self):
        for name in ("a.deb", "a.pkg.tar.zst", "a.rpm", "a.tar.xz", "a.AppImage"):
            p = os.path.join(self.d, name)
            open(p, "wb").write(b"x")
            self.assertEqual(c.main([p]), 2, name)

    def test_the_real_release_set_is_clean_when_it_exists(self):
        rel = os.path.join(HERE, "..", "build", "rc")
        paths = [os.path.join(rel, n) for n in os.listdir(rel) if n.endswith(".tar.gz")] if os.path.isdir(rel) else []
        if not paths:
            self.skipTest("no local release set")
        for p in paths:
            self.assertEqual(c.scan(p), [], p)


if __name__ == "__main__":
    unittest.main()
