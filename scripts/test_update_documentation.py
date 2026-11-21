#!/usr/bin/env python3
"""Regression test for the documentation of the launcher update (0.4.1).

The launcher CHECKS whether a newer version exists and shows a notice; it never downloads or installs anything, on Linux and on Windows. The documentation that ships with the project
must say so and must not describe the earlier automatic mechanisms (hosted Flatpak/OSTree update source, signed Flatpak updates, update portal, one-click update, background updater) as
something that exists. A line may MENTION such a mechanism only to deny it, to call it removed, shelved, development-only or historical. The documents of the early development
(documentation/*-2026-10-*.md, handovers) are not shipped; they carry a banner that marks them as historical (checked in the private repository, where they exist)."""
import os
import re
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, ".."))
SHIPPED = ["README.md", "documentation/user-guide.md", "documentation/security.md", "documentation/packaging.md", "documentation/architecture.md", "documentation/building-windows.md",
           "documentation/versioning-policy.md", "documentation/public-export.md", "documentation/release-pipeline.md", "documentation/building-from-source.md",
           "documentation/microsoft-components.md", "documentation/website.md"]

# the earlier mechanisms, described as if they existed
CLAIMS = re.compile(
    r"updates? (itself|automatically)|updated automatically|automatic(ally)? (launcher )?(update|download|install)|one-click update|update with one click|background (update|updater|install)|"
    r"update portal|CreateUpdateMonitor|hosted (Flatpak|OSTree)|flatpak remote-add|\.flatpakref|signed (OSTree|Flatpak) (repository|update)|Flatpak verifies updates|"
    r"flatpak update (org\.tswlauncher|--user org\.tswlauncher)|self-?updater|built-in updater|integrated updater", re.I)
# words that make a line a denial or a statement about the past / the development feature
DENIAL = re.compile(r"\b(no|not|never|nothing|without|neither|nor|removed|gone|shelved|dropped|historical|superseded|does not|doesn't|cannot|unpublished|development|dev-only|decided against|"
                    r"earlier|former|formerly|until|instead|since 0\.4\.1|from 0\.4\.1|refus\w*|forbid\w*|forbidden|prohibit\w*)\b", re.I)

HISTORICAL_BANNER = re.compile(r"historical|superseded|überholt|veraltet|historisch|obsolete", re.I)


def read(rel):
    return open(os.path.join(ROOT, rel), encoding="utf-8").read()


class ShippedDocumentation(unittest.TestCase):
    def test_no_shipped_document_describes_an_automatic_update_as_existing(self):
        found = []
        for rel in SHIPPED:
            if not os.path.exists(os.path.join(ROOT, rel)):
                continue
            for n, line in enumerate(read(rel).splitlines(), 1):
                # sentence by sentence: a denial in one sentence must not excuse a claim in the next
                for sentence in re.split(r"(?<=[.!?:;])\s+", line):
                    if CLAIMS.search(sentence) and not DENIAL.search(sentence):
                        found.append(f"{rel}:{n}: {sentence.strip()[:160]}")
        self.assertEqual(found, [], "a shipped document claims an automatic update or the removed update source; deny it or mark it as historical/development-only")

    def test_the_readme_and_the_user_guide_separate_the_check_from_the_manual_installation(self):
        readme, guide = read("README.md"), read("documentation/user-guide.md")
        for text in (readme, guide):
            self.assertRegex(text, r"(?i)never installs|downloads and installs nothing|Nothing is downloaded or installed")
            self.assertRegex(text, r"(?i)install(ing)? the (newer|new) (bundle|file|installer)|install it yourself|installing the newer file by hand")
        self.assertIn("checks for a newer version, but it never installs one", readme)
        self.assertIn("one plain HTTPS request", readme)
        self.assertIn("Open download page", guide)
        self.assertIn("Install only a NEWER bundle", readme)
        self.assertIn("Install only a NEWER bundle", guide)
        # the runtimes are not the launcher
        self.assertRegex(readme, r"`flatpak update` updates the Flatpak \*\*runtimes and extensions\*\*, not the launcher")

    def test_the_documents_name_the_two_addresses_the_notice_opens(self):
        notice = read("launcher/src/releasenotice.rs")
        self.assertIn("#download-linux", notice)
        self.assertIn("#download-windows", notice)
        self.assertIn("https://glaubenskrieger.github.io/TSW-Forever/", read("README.md"))
        self.assertIn("https://glaubenskrieger.github.io/TSW-Forever/", read("documentation/user-guide.md"))

    def test_the_documentation_matches_the_code_in_what_the_launcher_can_do(self):
        # what the documents promise, the shipped code must not contradict: the window program opens only the two download addresses and starts no installer
        main = read("launcher-gui/src/main.rs")
        self.assertIn("fn open_download_page(", main)
        for gone in ("flatpak install", "setup.exe", "/VERYSILENT", "CreateUpdateMonitor"):
            self.assertNotIn(gone, main)


class SwitchedOffIssueTracker(unittest.TestCase):
    """The project's GitHub issue tracker is switched off (owner decision 2026-11-21): nothing that ships may point at it or send users there."""

    PATTERN = re.compile(r"github\.com/[^\s)\"']*/issues|issue[ -]tracker|bugtracker|report (a )?(bug|problem)s? (through|via|on|at) (the )?(public |project )?(github )?issues", re.I)

    def test_no_shipped_document_metainfo_workflow_or_site_file_refers_to_the_issue_tracker(self):
        found = []
        files = [r for r in SHIPPED if os.path.exists(os.path.join(ROOT, r))] + ["CHANGELOG.md", "packaging/flatpak/org.tswlauncher.Launcher.metainfo.xml"]
        for base in ("website", os.path.join(".github", "workflows")):
            for d, _dirs, names in os.walk(os.path.join(ROOT, base)):
                files += [os.path.relpath(os.path.join(d, n), ROOT) for n in names]
        for rel in files:
            if rel.startswith("documentation/windows-development-progress") or rel.endswith("test_update_documentation.py"):
                continue
            for n, line in enumerate(read(rel).splitlines(), 1):
                if self.PATTERN.search(line):
                    found.append(f"{rel}:{n}: {line.strip()[:120]}")
        self.assertEqual(found, [])


class HistoricalDocuments(unittest.TestCase):
    """Only in the private development repository, where the early documents exist (they are not part of the public tree)."""

    STALE_TOPICS = re.compile(r"CreateUpdateMonitor|signed OSTree|OSTree repository|flatpakref|flatpak_sign|selfupdate\.rs|Update-Portal|automatisch aktualisiert|updated automatically", re.I)

    def test_a_document_that_still_describes_the_removed_update_mechanisms_carries_a_historical_banner(self):
        if not os.path.exists(os.path.join(ROOT, "AGENTS.md")):
            self.skipTest("the early development documents exist in the private repository only")
        shipped = {os.path.normpath(r) for r in SHIPPED} | {"documentation/windows-development-progress.md", "documentation/release-process.md", "documentation/flatpak-update-setup.md"}
        missing = []
        docs = os.path.join(ROOT, "documentation")
        for name in sorted(os.listdir(docs)):
            rel = os.path.normpath(os.path.join("documentation", name))
            if not name.endswith(".md") or rel in shipped:
                continue
            text = read(rel)
            if self.STALE_TOPICS.search(text) and not HISTORICAL_BANNER.search("\n".join(text.splitlines()[:8])):
                missing.append(rel)
        self.assertEqual(missing, [], "these documents describe removed update mechanisms without a historical banner in their first lines")


if __name__ == "__main__":
    unittest.main()
