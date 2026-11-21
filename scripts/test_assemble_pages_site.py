#!/usr/bin/env python3
"""The Pages site is assembled from web site + OSTree repository + install files, and none of them can overwrite another."""
import os
import sys
import tempfile
import unittest

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import assemble_pages_site as a  # noqa: E402

ID = a.APP_ID
COMMIT = "c" * 64


def w(path, text="x"):
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, "w") as f:
        f.write(text)


class Site(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp(prefix="tsw-pages-")
        j = lambda *p: os.path.join(self.d, *p)
        self.repo, self.refs, self.out = j("repo-in"), j("refs"), j("site")
        w(j("repo-in", "summary"), "s")
        w(j("repo-in", "objects", "ab", "cd.file"), "o")
        w(j("repo-in", "refs", "heads", "app", ID, "x86_64", "stable"), COMMIT + "\n")
        w(j("refs", "tsw-forever.flatpakrepo"), "[Flatpak Repo]")
        w(j("refs", f"{ID}.flatpakref"), "[Flatpak Ref]")
        self.web = j("web")
        w(j("web", "index.html"), "<h1>site</h1>")
        w(j("web", "style.css"), "body{}")

    def run_main(self, *extra):
        return a.main(["--repo", self.repo, "--refs", self.refs, "--out", self.out, *extra])

    def test_the_site_and_the_repository_live_side_by_side(self):
        self.assertEqual(self.run_main("--website", self.web), 0)
        o = lambda *p: os.path.join(self.out, *p)
        self.assertEqual(open(o("index.html")).read(), "<h1>site</h1>", "the web site's own index page is kept")
        self.assertTrue(os.path.isfile(o("style.css")) and os.path.isfile(o("repo", "summary")))
        self.assertTrue(os.path.isfile(o("repo", "refs", "heads", "app", ID, "x86_64", "stable")))
        self.assertTrue(os.path.isfile(o("tsw-forever.flatpakrepo")) and os.path.isfile(o(f"{ID}.flatpakref")))
        self.assertTrue(os.path.isfile(o(".nojekyll")))

    def test_without_a_web_site_a_minimal_index_page_is_written(self):
        self.assertEqual(self.run_main(), 0)
        self.assertIn("flatpak install", open(os.path.join(self.out, "index.html")).read())

    def test_a_web_site_cannot_take_a_name_of_the_repository(self):
        for name in ("repo", "channels.json", ".nojekyll", "tsw-forever.flatpakrepo", f"{ID}.flatpakref", "other.flatpakref"):
            w(os.path.join(self.web, name, "x") if name == "repo" else os.path.join(self.web, name))
            self.assertEqual(self.run_main("--website", self.web), 1, name)
            self.assertFalse(os.path.exists(self.out), "nothing is written when a collision is found")
            if name == "repo":
                import shutil
                shutil.rmtree(os.path.join(self.web, name))
            else:
                os.remove(os.path.join(self.web, name))

    def test_the_repository_must_be_complete_and_stable_only(self):
        os.remove(os.path.join(self.repo, "summary"))
        self.assertEqual(self.run_main(), 1)
        w(os.path.join(self.repo, "summary"))
        w(os.path.join(self.repo, "refs", "heads", "app", ID, "x86_64", "testing"), COMMIT)
        self.assertEqual(self.run_main(), 1, "a testing ref must never reach the public site")
        os.remove(os.path.join(self.repo, "refs", "heads", "app", ID, "x86_64", "testing"))
        os.remove(os.path.join(self.repo, "refs", "heads", "app", ID, "x86_64", "stable"))
        self.assertEqual(self.run_main(), 1, "no stable ref")
        w(os.path.join(self.repo, "refs", "heads", "app", ID, "x86_64", "stable"), "not-a-commit")
        self.assertEqual(self.run_main(), 1)

    def test_testing_install_files_or_records_are_refused(self):
        w(os.path.join(self.refs, f"{ID}-testing.flatpakref"))
        self.assertEqual(self.run_main(), 1)
        os.remove(os.path.join(self.refs, f"{ID}-testing.flatpakref"))
        rec = os.path.join(self.d, "channels.json")
        w(rec, '{"schema": 1, "channels": {"testing": {}}}')
        self.assertEqual(self.run_main("--channels-json", rec), 1)
        w(rec, '{"schema": 1, "channels": {"stable": {"version": "1.0.0", "commit": "' + COMMIT + '"}}}')
        self.assertEqual(self.run_main("--channels-json", rec), 0)
        self.assertTrue(os.path.isfile(os.path.join(self.out, "channels.json")))

    def test_links_and_a_used_output_folder_are_refused(self):
        os.symlink("/etc/passwd", os.path.join(self.web, "link"))
        self.assertEqual(self.run_main("--website", self.web), 1)
        os.remove(os.path.join(self.web, "link"))
        os.symlink("/etc/passwd", os.path.join(self.repo, "objects", "link"))
        self.assertEqual(self.run_main(), 1)
        os.remove(os.path.join(self.repo, "objects", "link"))
        w(os.path.join(self.out, "old.txt"))
        self.assertEqual(self.run_main(), 1, "an output folder that already holds something is refused")

    def test_oversized_files_and_missing_install_files_are_refused(self):
        big = os.path.join(self.repo, "objects", "big")
        with open(big, "wb") as f:
            f.truncate(a.FILE_LIMIT + 1)
        self.assertEqual(self.run_main(), 1)
        os.remove(big)
        os.remove(os.path.join(self.refs, "tsw-forever.flatpakrepo"))
        self.assertEqual(self.run_main(), 1)


class Preserve(unittest.TestCase):
    """A web site deployment alone keeps the published Flatpak data: assemble writes the manifest, preserve_live_site fetches and verifies, the site is assembled again."""

    def setUp(self):
        import http.server
        import threading
        import json
        Site.setUp(self)
        w(os.path.join(self.d, "channels.json"), "{}")
        self.assertEqual(a.main(["--repo", self.repo, "--refs", self.refs, "--out", self.out, "--website", self.web, "--channels-json", os.path.join(self.d, "channels.json")]), 0)
        self.manifest = json.load(open(os.path.join(self.out, "site-manifest.json")))
        root = self.out

        class H(http.server.SimpleHTTPRequestHandler):
            def __init__(self, *args, **kw):
                super().__init__(*args, directory=root, **kw)

            def log_message(self, *args):
                pass

            def list_directory(self, path):
                self.send_error(404)

        self.srv = http.server.ThreadingHTTPServer(("127.0.0.1", 0), H)
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()
        self.url = f"http://127.0.0.1:{self.srv.server_address[1]}"

    def tearDown(self):
        self.srv.shutdown()
        self.srv.server_close()

    def test_manifest_lists_the_flatpak_data_and_not_the_web_site(self):
        files = self.manifest["files"]
        self.assertIn("repo/summary", files)
        self.assertIn("channels.json", files)
        self.assertIn(f"{ID}.flatpakref", files)
        self.assertNotIn("index.html", files)
        self.assertNotIn("style.css", files)

    def test_round_trip_keeps_the_published_data_and_replaces_the_web_site(self):
        import preserve_live_site as p
        kept = os.path.join(self.d, "kept")
        self.assertEqual(p.main(["--site", self.url, "--out", kept]), 0)
        with open(os.path.join(self.web, "index.html"), "w") as f:
            f.write("<h1>new site</h1>")
        out2 = os.path.join(self.d, "site2")
        rc = a.main(["--repo", os.path.join(kept, "repo"), "--refs", os.path.join(kept, "refs"), "--channels-json", os.path.join(kept, "channels.json"), "--website", self.web, "--out", out2])
        self.assertEqual(rc, 0)
        self.assertEqual(open(os.path.join(out2, "index.html")).read(), "<h1>new site</h1>")
        for rel in self.manifest["files"]:
            self.assertEqual(open(os.path.join(out2, rel), "rb").read(), open(os.path.join(self.out, rel), "rb").read(), rel)

    def test_a_changed_live_file_is_refused(self):
        import preserve_live_site as p
        with open(os.path.join(self.out, "repo", "summary"), "w") as f:
            f.write("tampered")
        self.assertEqual(p.main(["--site", self.url, "--out", os.path.join(self.d, "kept")]), 1)

    def test_a_manifest_path_outside_the_flatpak_data_is_refused(self):
        import json
        import preserve_live_site as p
        bad = dict(self.manifest)
        bad["files"] = dict(bad["files"], **{"../x": "0" * 64})
        with open(os.path.join(self.out, "site-manifest.json"), "w") as f:
            json.dump(bad, f)
        self.assertEqual(p.main(["--site", self.url, "--out", os.path.join(self.d, "kept")]), 1)

    def test_no_manifest_is_an_error_and_http_elsewhere_is_refused(self):
        import preserve_live_site as p
        os.remove(os.path.join(self.out, "site-manifest.json"))
        self.assertEqual(p.main(["--site", self.url, "--out", os.path.join(self.d, "kept")]), 1)
        self.assertEqual(p.main(["--site", "http://example.org", "--out", os.path.join(self.d, "k2")]), 1)

    def test_website_only(self):
        out = os.path.join(self.d, "only")
        self.assertEqual(a.main(["--website-only", "--website", self.web, "--out", out]), 0)
        self.assertTrue(os.path.isfile(os.path.join(out, "index.html")) and os.path.isfile(os.path.join(out, ".nojekyll")))
        self.assertFalse(os.path.exists(os.path.join(out, "repo")))
        self.assertEqual(a.main(["--website-only", "--repo", self.repo, "--website", self.web, "--out", os.path.join(self.d, "o2")]), 1)


if __name__ == "__main__":
    unittest.main()
