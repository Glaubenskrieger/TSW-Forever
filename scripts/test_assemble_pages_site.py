#!/usr/bin/env python3
"""Tests of scripts/assemble_pages_site.py: the Pages site is the static web site only, never an installation file or a repository."""
import os
import shutil
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import assemble_pages_site as pages  # noqa: E402

ROOT = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))


class Site(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.web = os.path.join(self.tmp, "web")
        os.makedirs(os.path.join(self.web, "js"))
        for rel, text in (("index.html", "<html></html>"), ("js/app.js", "1"), ("style.css", "a{}")):
            with open(os.path.join(self.web, rel), "w") as f:
                f.write(text)
        self.out = os.path.join(self.tmp, "out")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def add(self, rel, data="x"):
        p = os.path.join(self.web, rel)
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(data)

    def test_the_web_site_is_copied_and_marked_for_pages(self):
        pages.assemble(self.web, self.out)
        self.assertTrue(os.path.isfile(os.path.join(self.out, "index.html")) and os.path.isfile(os.path.join(self.out, "js", "app.js")))
        self.assertTrue(os.path.isfile(os.path.join(self.out, ".nojekyll")), "Pages must not run Jekyll")
        self.assertEqual(sorted(os.listdir(self.out)), [".nojekyll", "index.html", "js", "style.css"])

    def test_the_real_web_site_assembles_and_is_small(self):
        total = pages.assemble(os.path.join(ROOT, "website"), self.out)
        self.assertLess(total, 1024 * 1024, "the whole web site is well below a megabyte")

    def test_installation_and_repository_files_are_refused(self):
        for rel in ("repo/summary", "summary", "channels.json", "site-manifest.json", "tsw-forever.flatpakrepo", "org.tswlauncher.Launcher.flatpakref", "x.flatpak", "setup.exe",
                    "tsw-forever-0.4.1-src.tar.gz", "key.gpg", "SHA256SUMS", "objects/ab/cd.filez", ".well-known/x.txt", "x.bin"):
            with self.subTest(rel):
                self.add(rel)
                with self.assertRaises(pages.SiteError):
                    pages.assemble(self.web, os.path.join(self.tmp, "o-" + rel.replace("/", "_")))
                p = os.path.join(self.web, rel)
                os.remove(p)
                for d in (os.path.dirname(p),):
                    if d != self.web and not os.listdir(d):
                        os.rmdir(d)

    def test_a_missing_site_a_link_a_big_file_and_a_used_output_are_refused(self):
        with self.assertRaises(pages.SiteError):
            pages.assemble(os.path.join(self.tmp, "nothing"), self.out)
        os.remove(os.path.join(self.web, "index.html"))
        with self.assertRaises(pages.SiteError):
            pages.assemble(self.web, self.out)
        self.add("index.html")
        os.symlink("/etc/passwd", os.path.join(self.web, "link.txt"))
        with self.assertRaises(pages.SiteError):
            pages.assemble(self.web, self.out)
        os.remove(os.path.join(self.web, "link.txt"))
        self.add("big.js", "x" * (pages.FILE_LIMIT + 1))
        with self.assertRaises(pages.SiteError):
            pages.assemble(self.web, self.out)
        os.remove(os.path.join(self.web, "big.js"))
        os.makedirs(self.out)
        self.add("../out/occupied.txt", "x")
        with self.assertRaises(pages.SiteError):
            pages.assemble(self.web, self.out)

    def test_cli_exit_codes(self):
        self.assertEqual(pages.main(["--website", self.web, "--out", self.out]), 0)
        self.assertEqual(pages.main(["--website", self.web, "--out", self.out]), 1, "the output folder is no longer empty")


class VerifyAssembled(unittest.TestCase):
    """--verify: the production deployment checks the artifact of the web site build with the assembler's own rules."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.site = os.path.join(self.tmp, "site")
        pages.assemble(os.path.join(ROOT, "website"), self.site)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def test_an_assembled_site_is_verified(self):
        self.assertGreater(pages.check_website(self.site, assembled=True), 0)
        self.assertEqual(pages.main(["--verify", self.site]), 0)

    def test_the_marker_is_required_and_must_be_empty(self):
        os.remove(os.path.join(self.site, ".nojekyll"))
        with self.assertRaises(pages.SiteError):
            pages.check_website(self.site, assembled=True)
        with open(os.path.join(self.site, ".nojekyll"), "w") as f:
            f.write("x")
        with self.assertRaises(pages.SiteError):
            pages.check_website(self.site, assembled=True)

    def test_the_same_forbidden_files_are_refused_in_an_assembled_site(self):
        for rel in ("setup.exe", "x.flatpak", "SHA256SUMS", "tsw-forever-0.4.1-src.tar.gz", "repo/summary", "key.gpg", ".secret", "x.bin"):
            with self.subTest(rel):
                p = os.path.join(self.site, rel)
                os.makedirs(os.path.dirname(p), exist_ok=True)
                with open(p, "w") as f:
                    f.write("x")
                self.assertEqual(pages.main(["--verify", self.site]), 1)
                os.remove(p)
                if os.path.dirname(p) != self.site:
                    os.rmdir(os.path.dirname(p))

    def test_a_plain_website_folder_is_not_an_assembled_site_and_verify_stands_alone(self):
        self.assertEqual(pages.main(["--verify", os.path.join(ROOT, "website")]), 1, "the source folder has no .nojekyll")
        with self.assertRaises(SystemExit):
            pages.main(["--verify", self.site, "--out", self.tmp])


if __name__ == "__main__":
    unittest.main()
