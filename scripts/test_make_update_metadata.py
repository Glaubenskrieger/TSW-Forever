#!/usr/bin/env python3
"""The metadata generator writes exactly the format the launcher parses (checked against the Rust fixture's shape) and refuses unusable input."""
import hashlib
import os
import re
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import make_update_metadata as m  # noqa: E402

REPO = "https://github.com/Glaubenskrieger/TSW-Forever"


class Metadata(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp(prefix="tsw-meta-")
        self.new = self.write("tsw-forever-0.3.5-windows-setup.exe", b"MZ" + b"new installer" * 10)
        self.old = self.write("tsw-forever-0.3.4-windows-setup.exe", b"MZ" + b"old installer" * 10)

    def write(self, name, data):
        p = os.path.join(self.d, name)
        with open(p, "wb") as f:
            f.write(data)
        return p

    def test_the_text_has_the_exact_launcher_format(self):
        text = m.build("0.3.5", self.new, "0.3.4", self.old, 180, REPO, "2026-12-01T10:00:00Z")
        lines = text.splitlines()
        self.assertEqual(lines[0], "tsw-forever-update 1")
        self.assertTrue(text.endswith("\n") and "\r" not in text)
        keys = [l.split(": ", 1)[0] for l in lines[1:]]
        self.assertEqual(keys, ["channel", "version", "issued", "expires", "windows-installer", "windows-url", "windows-size", "windows-sha256",
                                "previous-version", "previous-installer", "previous-url", "previous-size", "previous-sha256", "notes-url"])
        d = dict(l.split(": ", 1) for l in lines[1:])
        self.assertEqual(d["channel"], "stable")
        self.assertEqual((d["issued"], d["expires"]), ("2026-12-01T10:00:00Z", "2027-05-30T10:00:00Z"))
        self.assertEqual(d["windows-url"], f"{REPO}/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe")
        self.assertEqual(d["windows-sha256"], hashlib.sha256(open(self.new, "rb").read()).hexdigest())
        self.assertEqual(int(d["windows-size"]), os.path.getsize(self.new))
        self.assertEqual(d["previous-url"], f"{REPO}/releases/download/v0.3.4/tsw-forever-0.3.4-windows-setup.exe")
        self.assertTrue(all(32 <= ord(c) < 127 or c == "\n" for c in text), "printable ASCII only")
        self.assertRegex(d["windows-sha256"], r"^[0-9a-f]{64}$")

    def test_the_keys_and_their_order_equal_the_rust_fixture_that_the_launcher_tests_parse(self):
        rust = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "launcher", "src", "update", "fixtures.rs")).read()
        block = re.search(r'pub const GOOD_META: &str = r#"(.*?)"#;', rust, re.S).group(1)
        fixture_keys = [l.split(": ", 1)[0] for l in block.splitlines()[1:] if l != "flatpak-ref" and not l.startswith("flatpak-ref")]
        text = m.build("0.3.5", self.new, "0.3.4", self.old, 180, REPO, "2026-12-01T10:00:00Z")
        ours = [l.split(": ", 1)[0] for l in text.splitlines()[1:]]
        self.assertEqual(ours, fixture_keys)
        self.assertEqual(text.splitlines()[0], block.splitlines()[0])

    def test_a_release_without_a_previous_version_is_fine(self):
        text = m.build("0.3.5", self.new, repo_url=REPO, issued="2026-12-01T10:00:00Z")
        self.assertNotIn("previous-", text)

    def test_unusable_input_is_refused(self):
        bad = [
            dict(version="0.3", installer=self.new), dict(version="0.3.5-rc.1", installer=self.new), dict(version="v0.3.5", installer=self.new),
            dict(version="0.3.6", installer=self.new),  # file name does not match the version
            dict(version="0.3.5", installer=self.new, valid_days=0), dict(version="0.3.5", installer=self.new, valid_days=401),
            dict(version="0.3.5", installer=self.new, previous_version="0.3.4"),
            dict(version="0.3.5", installer=self.new, previous_version="0.3.5", previous_installer=self.new),
            dict(version="0.3.5", installer=self.new, previous_version="0.4.0", previous_installer=self.old),
            dict(version="0.3.5", installer=self.new, repo_url="http://github.com/x/y"),
            dict(version="0.3.5", installer=self.new, flatpak_ref="app/x/x86_64/testing"),
            dict(version="0.3.5", installer=self.write("tsw-forever-0.3.5-windows-setup.exe", b"not a program")),
            dict(version="0.3.5", installer=self.write("tsw-forever-0.3.5-windows-setup.exe", b"")),
        ]
        for kw in bad:
            with self.assertRaises(ValueError, msg=repr({k: v for k, v in kw.items() if k != "installer"})):
                m.build(**kw)

    def test_the_command_line_writes_the_file_and_reports_errors(self):
        out = os.path.join(self.d, "update-stable.txt")
        self.assertEqual(m.main(["--version", "0.3.5", "--installer", self.new, "--repo-url", REPO, "--out", out]), 0)
        self.assertTrue(open(out).read().startswith("tsw-forever-update 1\n"))
        self.assertEqual(m.main(["--version", "0.3", "--installer", self.new, "--out", out + "2"]), 1)
        self.assertFalse(os.path.exists(out + "2"))


if __name__ == "__main__":
    unittest.main()
