#!/usr/bin/env python3
"""Tests of scripts/check_release_set.py with a synthetic, consistent release set. The official Linux release is exactly: Flatpak bundle, source archive, SHA256SUMS."""
import hashlib
import io
import json
import os
import sys
import tarfile
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
import check_release_set as c  # noqa: E402

V = "0.2.0-rc.1"


def sha(p):
    return hashlib.sha256(open(p, "rb").read()).hexdigest()


def make_src(path, version, app_version=None, drop=(), top=None):
    top = top or f"tsw-forever-{version}"
    with tarfile.open(path, "w:gz") as t:
        for rel in c.REQUIRED_IN_SOURCE:
            if rel in drop:
                continue
            fixture_notice = b"Synthetic test licence notice\n"
            fixture_inventory = {'format': 1, 'target': 'x86_64-unknown-linux-gnu', 'additional_notices': [], 'components': [
                {'name': 'fixture', 'version': '1.0.0', 'license': 'MIT', 'source_sha256': 'a' * 64,
                 'source_url': 'https://static.crates.io/crates/fixture/fixture-1.0.0.crate',
                 'notices': [{'path': 'LICENSES/fixture.txt', 'sha256': hashlib.sha256(fixture_notice).hexdigest(), 'origin': 'synthetic fixture'}]}]}
            special = {
                'LICENSES/inventory.json': json.dumps(fixture_inventory).encode(),
                'launcher-gui/Cargo.lock': ('[[package]]\nname="fixture"\nversion="1.0.0"\nchecksum="' + 'a' * 64 + '"\n').encode(),
                'packaging/project.conf': f"APP_VERSION={app_version or version}\n".encode()}
            data = special.get(rel, b"x\n")
            ti = tarfile.TarInfo(f"{top}/{rel}")
            ti.size = len(data)
            t.addfile(ti, io.BytesIO(data))
        ti = tarfile.TarInfo(f"{top}/LICENSES/fixture.txt")
        ti.size = len(fixture_notice)
        t.addfile(ti, io.BytesIO(fixture_notice))


class Set(unittest.TestCase):
    def build(self, **over):
        d = tempfile.mkdtemp()
        names = c.expected_names(V)
        with open(os.path.join(d, names["flatpak"]), "wb") as f:
            f.write(b"\0" * (2 * 1024 * 1024))
        make_src(os.path.join(d, names["src"]), V, over.get("src_app_version"), over.get("drop", ()), over.get("top"))
        lines = [f"{sha(os.path.join(d, n))}  {n}" for n in (names["flatpak"], names["src"])]
        if over.get("bad_sums"):
            lines[0] = "0" * 64 + lines[0][64:]
        if over.get("drop_sum"):
            lines.pop()
        for extra in over.get("extra_sum_lines", ()):
            lines.append(extra)
        if over.get("windows"):
            wn = c.expected_names(V, True)
            with open(os.path.join(d, wn["windows-setup"]), "wb") as f:
                f.write(b"MZ" + b"\0" * (1024 * 1024))
            lines += [f"{sha(os.path.join(d, n))}  {n}" for n in (wn["windows-setup"],)]
        open(os.path.join(d, "SHA256SUMS"), "w").write("\n".join(lines) + "\n")
        for extra in over.get("extra_files", ()):
            os.makedirs(os.path.dirname(os.path.join(d, extra)), exist_ok=True)
            open(os.path.join(d, extra), "wb").write(b"x")
        return d

    def test_a_consistent_set_passes(self):
        self.assertEqual(c.check(self.build(), V), [])

    def test_the_combined_set_needs_the_windows_files_of_the_same_version(self):
        self.assertEqual(c.check(self.build(windows=True), V, True), [])
        self.assertTrue(any("missing windows-setup" in e for e in c.check(self.build(), V, True)), "a Linux-only set is not a combined release")
        self.assertTrue(any("unexpected file" in e for e in c.check(self.build(windows=True), V, False)), "the Linux set check still refuses Windows files")
        d = self.build(windows=True)
        os.remove(os.path.join(d, c.expected_names(V, True)["windows-setup"]))
        self.assertTrue(any("missing windows-setup" in e for e in c.check(d, V, True)))
        self.assertTrue(any("unexpected file" in e for e in c.check(self.build(windows=True, extra_files=["tsw-forever-%s-windows-portable.zip" % V]), V, True)), "a portable zip is not part of any release")

    def test_the_names_derive_from_the_version(self):
        self.assertEqual(c.expected_names(V), {"flatpak": "tsw-forever-0.2.0-rc.1.flatpak", "src": "tsw-forever-0.2.0-rc.1-src.tar.gz", "sums": "SHA256SUMS"})

    def test_a_missing_artifact_is_reported(self):
        for victim in ("flatpak", "src", "sums"):
            d = self.build()
            os.remove(os.path.join(d, c.expected_names(V)[victim]))
            self.assertTrue(any("missing" in e for e in c.check(d, V)), victim)

    def test_only_the_flatpak_bundle_and_the_source_archive_are_official_artifacts(self):
        for extra in ("tsw-forever_0.2.0~rc.1-1_amd64.deb", "tsw-forever-0.2.0rc1-1-x86_64.pkg.tar.zst", "tsw-forever-0.2.0-1.x86_64.rpm", "PKGBUILD",
                      "TSW-Forever.AppImage", "arch-package/x.pkg.tar.zst", "local-test/x"):
            errs = c.check(self.build(extra_files=[extra]), V)
            self.assertTrue(any("unexpected file" in e for e in errs), (extra, errs))

    def test_checksum_problems_are_reported(self):
        self.assertTrue(any("wrong hash" in e for e in c.check(self.build(bad_sums=True), V)))
        self.assertTrue(any("does not list" in e for e in c.check(self.build(drop_sum=True), V)))
        errs = c.check(self.build(extra_sum_lines=["a" * 64 + "  PKGBUILD"]), V)
        self.assertTrue(any("not an artifact" in e for e in errs), errs)

    def test_the_source_archive_must_match_the_version(self):
        self.assertTrue(any("APP_VERSION" in e for e in c.check(self.build(src_app_version="0.3.0"), V)))
        self.assertTrue(any("top folder" in e for e in c.check(self.build(top="other-0.2.0"), V)))

    def test_the_source_archive_must_carry_everything_needed_to_build(self):
        for rel in c.REQUIRED_IN_SOURCE:
            if rel == "packaging/project.conf":
                continue
            errs = c.check(self.build(drop=(rel,)), V)
            self.assertTrue(any(rel in e and "lacks" in e for e in errs), rel)
        for needed in ("launcher/Cargo.lock", "launcher-gui/Cargo.lock", "patcher/Cargo.lock", "launcher-gui/ui/launcher.slint", "documentation/building-from-source.md", "LICENSE"):
            self.assertIn(needed, c.REQUIRED_IN_SOURCE)

    def test_a_tiny_flatpak_bundle_is_suspicious(self):
        d = self.build()
        open(os.path.join(d, c.expected_names(V)["flatpak"]), "wb").write(b"flatpak")
        errs = c.check(d, V)
        self.assertTrue(any("small" in e for e in errs) and any("wrong hash" in e for e in errs), errs)

    def test_the_main_function_sets_the_exit_code(self):
        self.assertEqual(c.main([self.build(), "--version", V]), 0)
        self.assertEqual(c.main([self.build(bad_sums=True), "--version", V]), 1)
        self.assertEqual(c.main([]), 2)
        self.assertEqual(c.main([self.build(), "--no-arch-package"]), 2, "the Arch option is gone with the Arch package")

    def test_the_real_release_set_is_consistent_when_it_exists(self):
        rel = os.path.join(HERE, "..", "build", "rc")
        if not os.path.isdir(rel) or not os.path.isfile(os.path.join(rel, "SHA256SUMS")):
            self.skipTest("no local release set in build/rc")
        if not any(n.startswith(f"tsw-forever-{c.app_version()}") for n in os.listdir(rel)):
            self.skipTest("build/rc holds a historical candidate set of another version (retained as audit evidence)")
        self.assertEqual(c.check(rel, c.app_version()), [])


if __name__ == "__main__":
    unittest.main()
