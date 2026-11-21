#!/usr/bin/env python3
"""Tests of scripts/release_candidate.py (manifest of the artifacts of one commit)."""
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import release_candidate as rc  # noqa: E402

V = "0.4.0"
SHA = "a" * 40


class Candidate(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.parts = os.path.join(self.tmp, "parts")
        for kind, files in rc.expected(V).items():
            d = os.path.join(self.parts, f"tsw-forever-{V}-{kind}-{SHA}")
            os.makedirs(d)
            for n in files:
                with open(os.path.join(d, n), "w") as f:
                    f.write(f"data {kind} {n}\n")
        self.manifest = os.path.join(self.tmp, "candidate.json")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def part(self, kind, name):
        return os.path.join(self.parts, f"tsw-forever-{V}-{kind}-{SHA}", name)

    def test_create_and_verify(self):
        rc.create(self.parts, V, SHA, self.manifest)
        rc.verify(self.parts, self.manifest, V, SHA)

    def test_changed_byte_is_refused(self):
        rc.create(self.parts, V, SHA, self.manifest)
        with open(self.part("windows", f"tsw-forever-{V}-windows-setup.exe"), "a") as f:
            f.write("x")
        with self.assertRaises(rc.CandidateError):
            rc.verify(self.parts, self.manifest, V, SHA)

    def test_missing_and_extra_files_are_refused(self):
        os.remove(self.part("source", "SHA256SUMS-source"))
        with self.assertRaises(rc.CandidateError):
            rc.create(self.parts, V, SHA, self.manifest)
        open(self.part("source", "SHA256SUMS-source"), "w").write("x")
        open(self.part("source", "extra"), "w").write("x")
        with self.assertRaises(rc.CandidateError):
            rc.create(self.parts, V, SHA, self.manifest)

    def test_other_commit_or_version_is_refused(self):
        rc.create(self.parts, V, SHA, self.manifest)
        with self.assertRaises(rc.CandidateError):
            rc.verify(self.parts, self.manifest, V, "c" * 40)
        with self.assertRaises(rc.CandidateError):
            rc.verify(self.parts, self.manifest, "0.4.1", SHA)

    def test_missing_artifact_folder_and_bad_ids(self):
        shutil.rmtree(os.path.join(self.parts, f"tsw-forever-{V}-windows-{SHA}"))
        with self.assertRaises(rc.CandidateError):
            rc.create(self.parts, V, SHA, self.manifest)
        with self.assertRaises(rc.CandidateError):
            rc.scan(self.parts, "x", SHA)
        with self.assertRaises(rc.CandidateError):
            rc.scan(self.parts, V, "abc")

    def test_empty_file_and_symlink_are_refused(self):
        open(self.part("windows", "SHA256SUMS-windows"), "w").close()
        with self.assertRaises(rc.CandidateError):
            rc.create(self.parts, V, SHA, self.manifest)
        os.remove(self.part("windows", "SHA256SUMS-windows"))
        os.symlink(self.part("source", "SHA256SUMS-source"), self.part("windows", "SHA256SUMS-windows"))
        with self.assertRaises(rc.CandidateError):
            rc.create(self.parts, V, SHA, self.manifest)

    def test_cli_exit_codes(self):
        self.assertEqual(rc.main(["create", self.parts, "--version", V, "--commit", SHA, "--out", self.manifest]), 0)
        self.assertEqual(rc.main(["verify", self.parts, self.manifest, "--version", V, "--commit", SHA]), 0)
        self.assertEqual(rc.main(["verify", self.parts, self.manifest, "--version", V, "--commit", "d" * 40]), 1)


if __name__ == "__main__":
    unittest.main()
