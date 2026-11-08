#!/usr/bin/env python3
"""The distribution decision of 2026-11-02 as a test: on Linux the project offers exactly ONE precompiled package, the Flatpak, plus the source archive and the Git
source. No Debian/Ubuntu package, no Arch package or PKGBUILD, no RPM, no AppImage, no distribution-specific build, no container image for such builds. Windows has its own
per-user installer (packaging/windows, decision of 2026-11-05) and its code must not be removed. Needles are assembled so this file does not match itself."""
import os
import re
import sys
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, ".."))
sys.path.insert(0, HERE)
import check_flatpak_package as c  # noqa: E402

WORKFLOWS = os.path.join(ROOT, ".github", "workflows")
SKIP_DIRS = {".git", "build", "target", "analysis", "reports", "logs", "tools", "native", "node_modules", "__pycache__", "vendor"}
FORMAT_WORDS = [w[0] + w[1:] for w in ("deb" + "ian", "p" + "kgbuild", "make" + "pkg", "dp" + "kg", "pac" + "man", "arch" + "linux", "app" + "image", "r" + "pm")]


def read(rel):
    return open(os.path.join(ROOT, rel), encoding="utf-8").read()


def tracked_files():
    for d, dirs, files in os.walk(ROOT):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
        for f in files:
            yield os.path.relpath(os.path.join(d, f), ROOT)


class OnlyFlatpakAndSource(unittest.TestCase):
    def test_the_packaging_folder_holds_only_the_flatpak_the_windows_installer_and_the_project_configuration(self):
        # Windows (decision of 2026-11-05) is a separate operating system with its own per-user installer; it is not a Linux distribution package
        self.assertEqual(sorted(os.listdir(os.path.join(ROOT, "packaging"))), ["flatpak", "project.conf", "windows"])
        self.assertEqual(sorted(os.listdir(os.path.join(ROOT, "packaging", "windows"))), ["tsw-forever.iss"])

    def test_no_recipe_or_build_script_for_a_distribution_package_exists(self):
        names = {os.path.basename(f).lower() for f in tracked_files()}
        for forbidden in ("pkg" + "build", "build_" + "deb.sh", "ci_" + "build.sh", "build_in_container.sh", "dockerfile", "containerfile"):
            self.assertNotIn(forbidden, names)
        for f in tracked_files():
            self.assertFalse(re.search(r"\.(spec|deb|rpm|appimage|pkg\.tar\.(zst|xz|gz))$", f, re.I), f)
            self.assertFalse(re.match(r"(packaging|debian)/(debian|arch|rpm|aur)(/|$)", f), f)

    def test_no_workflow_builds_or_publishes_a_distribution_package_or_uses_a_container_build(self):
        for name in sorted(os.listdir(WORKFLOWS)):
            code = c._code_only(read(f".github/workflows/{name}"))
            self.assertIsNone(re.search(c.RC_FORBIDDEN_FORMATS, code, re.I), name)

    def test_the_build_scripts_know_no_distribution_package(self):
        for rel in ("scripts/build_flatpak.sh", "scripts/native_stage.sh", "scripts/install_native.sh", "scripts/check_release_set.py", "scripts/check_release_artifacts.py", "scripts/version.py"):
            text = read(rel).lower()
            for word in FORMAT_WORDS:
                self.assertNotIn(word, text, f"{rel} mentions {word}")

    def test_the_documentation_offers_no_distribution_package(self):
        for rel in ("README.md", "documentation/packaging.md", "documentation/building-from-source.md"):
            text = read(rel)
            for cmd in ("apt install ./", "dpkg -i", "pacman -U", "makepkg", "dnf install ./", "rpm -i"):
                self.assertNotIn(cmd, text, f"{rel}: {cmd}")
        readme = read("README.md")
        self.assertIn("Flatpak", readme)
        self.assertRegex(readme, r"(?i)source")
        self.assertRegex(read("documentation/packaging.md"), r"(?i)no .*(deb|rpm|distribution-specific)")

    def test_the_source_archive_is_the_second_artifact_and_has_its_build_instructions(self):
        self.assertTrue(os.path.isfile(os.path.join(ROOT, "documentation", "building-from-source.md")))
        self.assertTrue(os.path.isfile(os.path.join(ROOT, "scripts", "native_stage.sh")))
        stage = read("scripts/native_stage.sh")
        self.assertIn("building-from-source.md", stage)
        self.assertNotIn("packaging/" + "debian", stage)

    def test_windows_stays_on_the_roadmap_and_its_groundwork_in_the_code_is_not_removed(self):
        self.assertRegex(read("README.md"), r"(?i)windows[^\n]*(installer|roadmap|later)|(installer|roadmap|later)[^\n]*windows")
        self.assertIn("cfg!(windows)", read("launcher/src/runner.rs"))
        self.assertIn("cfg!(windows)", read("launcher/src/main.rs"))
        self.assertIn("Native", read("launcher/src/runner.rs"))


if __name__ == "__main__":
    unittest.main()
