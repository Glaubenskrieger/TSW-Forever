#!/usr/bin/env python3
"""Tests for scripts/version.py and for the consistency of the real project files (no network, no Git)."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import version as v  # noqa: E402

# the native research crate is private and absent from a checkout of the public file set: it is checked only when it exists (as in version.py)
FILES = [v.CONF, v.METAINFO, v.ENGINE_TOML, v.MANIFEST_RS, *v.APP_CRATES, *v.LOCKS, *v.ENGINE_LOCKS]
if os.path.exists(os.path.join(v.ROOT, v.RDB_TOML)):
    FILES.append(v.RDB_TOML)


def read(rel):
    with open(os.path.join(v.ROOT, rel), encoding="utf-8") as f:
        return f.read()


class Syntax(unittest.TestCase):
    def test_valid_and_invalid_versions(self):
        for ok in ["0.2.0-dev.1", "0.3.0-alpha.1", "0.3.0-beta.1", "0.3.0-rc.1", "0.3.0", "1.0.0", "1.2.3+build.5", "1.0.0-x-y-z.--"]:
            v.parse(ok)
        for bad in ["", "1", "1.2", "01.2.3", "1.02.3", "1.2.3-", "1.2.3-01", "1.2.3+", "v1.2.3", "1.2.3.4", "1.2.3-α", " 1.2.3", "1.2.3\n", None, 3]:
            with self.assertRaises(ValueError, msg=repr(bad)):
                v.parse(bad)

    def test_precedence_follows_the_semver_chain(self):
        chain = ["1.0.0-alpha", "1.0.0-alpha.1", "1.0.0-alpha.beta", "1.0.0-beta", "1.0.0-beta.2", "1.0.0-beta.11", "1.0.0-rc.1", "1.0.0", "1.0.1", "1.1.0", "2.0.0"]
        for i, a in enumerate(chain):
            for j, b in enumerate(chain):
                self.assertEqual(v.compare(a, b), (i > j) - (i < j), (a, b))

    def test_project_style_prereleases_order_correctly(self):
        order = ["0.2.0-alpha.0", "0.2.0-alpha.1", "0.2.0-alpha.10", "0.2.0-beta.1", "0.2.0-rc.1", "0.2.0", "0.2.1", "0.3.0-alpha.0", "0.3.0"]
        for a, b in zip(order, order[1:]):
            self.assertEqual(v.compare(a, b), -1, (a, b))
            self.assertEqual(v.compare(b, a), 1)

    def test_project_versions_are_plain_numeric(self):
        # SemVer would sort "dev" above "alpha"/"beta"; the project avoids the problem by allowing no suffix at all
        self.assertEqual(v.compare("0.2.0-dev.1", "0.2.0-alpha.1"), 1)
        for ok in ["0.2.0", "0.3.0", "1.0.0", "0.10.12"]:
            self.assertTrue(v.phase_ok(ok), ok)
        for bad in ["0.2.0-dev.1", "0.2.0-alpha.0", "0.2.0-beta.1", "0.2.0-rc.1", "0.2.0-rc.6", "1.2.3+build.5"]:
            self.assertFalse(v.phase_ok(bad), bad)

    def test_build_metadata_is_ignored_and_numbers_are_numeric(self):
        self.assertEqual(v.compare("1.0.0+a", "1.0.0+b"), 0)
        self.assertEqual(v.compare("0.10.0", "0.9.0"), 1, "numeric, not textual")
        self.assertTrue(v.is_prerelease("0.2.0-dev.1") and not v.is_prerelease("0.2.0"))


class RealProject(unittest.TestCase):
    def test_all_places_agree(self):
        self.assertEqual(v.check(), [])

    def test_ci_can_read_the_version_and_the_script_agrees_with_cargo(self):
        out = subprocess.run([sys.executable, os.path.join(v.ROOT, "scripts/version.py"), "show"], capture_output=True, text=True, check=True).stdout.strip()
        v.parse(out)
        self.assertIn(f'version = "{out}"', read("launcher/Cargo.toml"))
        self.assertIn(f'version = "{out}"', read("launcher-gui/Cargo.toml"))
        self.assertIn(f'release version="{out}"', read(v.METAINFO))

    def test_workflows_check_and_use_the_version(self):
        for wf in ("flatpak.yml", "release-flatpak.yml"):
            text = read(f".github/workflows/{wf}")
            self.assertIn("scripts/version.py check", text, wf)
            self.assertIn("scripts/version.py show", text, wf)
            self.assertIn("steps.version.outputs.version", text, wf)

    def test_manifest_format_is_a_plain_integer_and_not_a_version_of_anything_else(self):
        fmt = v.conf_value("PATCH_MANIFEST_FORMAT")
        self.assertRegex(fmt, r"^[1-9]\d*$")
        self.assertNotEqual(fmt, v.conf_value("APP_VERSION"))
        self.assertIn(f'"tsw-forever-patch {fmt}"', read(v.MANIFEST_RS))

    def test_independent_components_are_valid_semver_and_not_forced_equal(self):
        for rel in (v.ENGINE_TOML, v.RDB_TOML):
            if os.path.exists(os.path.join(v.ROOT, rel)):
                v.parse(v.toml_version(rel))
        self.assertNotEqual(v.toml_version(v.ENGINE_TOML), v.conf_value("APP_VERSION"))


class Changelog(unittest.TestCase):
    def test_no_invented_releases_and_an_unreleased_section(self):
        import re
        text = read("CHANGELOG.md")
        self.assertIn("## [Unreleased]", text)
        released = re.findall(r"^## \[(\d[^\]]*)\]", text, re.M)
        # a released section must name a valid version and carry a date; as long as nothing was published there must be none
        for r in released:
            v.parse(r)
            self.assertRegex(text, r"## \[%s\] – \d{4}-\d{2}-\d{2}" % re.escape(r))


class Tool(unittest.TestCase):
    def setUp(self):
        self.root = tempfile.mkdtemp(prefix="tsw-version-")
        for rel in FILES:
            os.makedirs(os.path.dirname(os.path.join(self.root, rel)), exist_ok=True)
            shutil.copy(os.path.join(v.ROOT, rel), os.path.join(self.root, rel))

    def tearDown(self):
        shutil.rmtree(self.root, ignore_errors=True)

    def test_set_updates_every_place_and_check_notices_drift(self):
        v.set_version("0.3.1", "2027-01-02", self.root)
        self.assertEqual(v.check(self.root), [])
        self.assertEqual(v.conf_value("APP_VERSION", self.root), "0.3.1")
        self.assertEqual(v.metainfo_release(self.root), ("0.3.1", "stable"))
        v.set_version("0.3.0", "2027-02-03", self.root)
        self.assertEqual(v.check(self.root), [])
        self.assertEqual(v.metainfo_release(self.root), ("0.3.0", "stable"))
        with open(os.path.join(self.root, "launcher/Cargo.toml"), "a+") as f:
            f.seek(0)
            t = f.read()
        with open(os.path.join(self.root, "launcher/Cargo.toml"), "w") as f:
            f.write(t.replace('version = "0.3.0"', 'version = "0.3.1"', 1))
        errs = v.check(self.root)
        self.assertTrue(any("launcher/Cargo.toml" in e for e in errs), errs)

    def test_set_refuses_invalid_versions_and_changes_nothing(self):
        before = open(os.path.join(self.root, v.CONF)).read()
        for bad in ["1.0", "v1.0.0", "1.0.0-01", "0.3.0-rc.1", "0.3.0-alpha.1", "0.3.0+b1"]:
            with self.assertRaises(ValueError):
                v.set_version(bad, None, self.root)
        self.assertEqual(open(os.path.join(self.root, v.CONF)).read(), before)

    def test_a_final_version_with_a_development_release_type_is_flagged(self):
        v.set_version("0.3.0", "2027-02-03", self.root)
        p = os.path.join(self.root, v.METAINFO)
        t = open(p).read().replace('type="stable"', 'type="development"')
        open(p, "w").write(t)
        self.assertTrue(any("type" in e for e in v.check(self.root)))

    def test_a_manifest_format_that_is_not_an_integer_is_flagged(self):
        p = os.path.join(self.root, v.CONF)
        text = open(p).read()
        open(p, "w").write(text.replace("PATCH_MANIFEST_FORMAT=1", "PATCH_MANIFEST_FORMAT=1.2.0"))
        self.assertTrue(any("PATCH_MANIFEST_FORMAT" in e for e in v.check(self.root)))


if __name__ == "__main__":
    unittest.main()
