#!/usr/bin/env python3
"""scripts/check_flatpak_live.sh against a LOCAL http server (loopback only) that serves a site assembled the way the pipeline assembles it, with throw-away keys.
Every defect of the live source that the pipeline must catch before it publishes the release is tried: a missing file, a wrong key, a wrong repository address, an unsigned
repository, a stale commit."""
import http.server
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import unittest
import functools

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from testtools import requires_tools  # noqa: E402
KEYS = os.path.join(HERE, "flatpak_keys.sh")
REFS = os.path.join(HERE, "gen_flatpak_refs.sh")
ASSEMBLE = os.path.join(HERE, "assemble_pages_site.py")
CHECK = os.path.join(HERE, "check_flatpak_live.sh")
APP = "org.tswlauncher.Launcher"
REPO_URL = "https://glaubenskrieger.github.io/TSW-Forever/repo"  # the address in packaging/project.conf, which the install files name


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


class Quiet(http.server.SimpleHTTPRequestHandler):
    def log_message(self, *a):
        pass

    def list_directory(self, path):  # a real Pages site answers 404 for a folder without an index page
        self.send_error(404, "No permission to list directory")
        return None


@requires_tools("flatpak", "gpg", "ostree", "curl", "python3")
class LiveCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-live-")
        cls.keys = []
        for n in ("a", "b"):
            home = os.path.join(cls.d, "gnupg-" + n)
            fpr = sh(KEYS, "generate-split", home, f"TSW live throw-away {n} <{n}@example.invalid>").stdout.strip().splitlines()[-1]
            pub = os.path.join(cls.d, f"pub-{n}.gpg")
            assert sh(KEYS, "export-public", home, fpr, pub).returncode == 0
            cls.keys.append((home, fpr, pub))
        cls.repo = os.path.join(cls.d, "repo")
        export(cls.d, "app1", cls.repo, "one")
        assert sh(KEYS, "sign-repo", cls.keys[0][0], cls.keys[0][1], cls.repo).returncode == 0
        cls.commit = subprocess.run(["ostree", "rev-parse", f"--repo={cls.repo}", f"app/{APP}/x86_64/stable"], capture_output=True, text=True).stdout.strip()
        cls.refs = os.path.join(cls.d, "refs")
        assert sh(REFS, cls.refs, cls.keys[0][2]).returncode == 0
        cls.site = cls.build_site("site")

    @classmethod
    def build_site(cls, name, repo=None, refs=None):
        out = os.path.join(cls.d, name)
        site = os.path.join(cls.d, name + "-web")
        os.makedirs(site, exist_ok=True)
        with open(os.path.join(site, "index.html"), "w") as f:
            f.write("<!doctype html><title>x</title>")
        r = subprocess.run([sys.executable, "-I", ASSEMBLE, "--repo", repo or cls.repo, "--refs", refs or cls.refs, "--website", site, "--out", out], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr
        return out

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.d, ignore_errors=True)

    def serve(self, folder):
        handler = functools.partial(Quiet, directory=folder)
        srv = socketserver.TCPServer(("127.0.0.1", 0), handler)
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        self.addCleanup(srv.server_close)
        self.addCleanup(srv.shutdown)
        return f"http://127.0.0.1:{srv.server_address[1]}"

    def check(self, folder, *, pub=None, commit=None, extra=()):
        url = self.serve(folder)
        return sh(CHECK, "--site", url, "--pub", pub or self.keys[0][2], "--expect-commit", commit or self.commit, "--allow-loopback-http", "--retries", "1", "--interval", "0", *extra)

    def copy_site(self):
        c = os.path.join(self.d, "c" + str(len(os.listdir(self.d))))
        shutil.copytree(self.site, c)
        return c

    def test_a_complete_signed_site_passes(self):
        r = self.check(self.site)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("live source verified", r.stdout)

    def test_every_missing_file_is_caught(self):
        for rel in ("repo/summary", "repo/summary.sig", "tsw-forever.flatpakrepo", f"{APP}.flatpakref", "index.html"):
            c = self.copy_site()
            os.remove(os.path.join(c, rel))
            r = self.check(c)
            self.assertEqual(r.returncode, 1, rel)
            self.assertIn("not reachable", r.stderr, rel)

    def test_a_site_signed_with_another_key_or_unsigned_is_caught(self):
        r = self.check(self.site, pub=self.keys[1][2])
        self.assertEqual(r.returncode, 1)
        self.assertIn("public key", r.stderr)
        unsigned = os.path.join(self.d, "unsigned-repo")
        export(self.d, "app-u", unsigned, "u")
        commit = subprocess.run(["ostree", "rev-parse", f"--repo={unsigned}", f"app/{APP}/x86_64/stable"], capture_output=True, text=True).stdout.strip()
        site = self.build_site("site-unsigned", repo=unsigned)
        r = self.check(site, commit=commit)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not verify", r.stderr)

    def test_a_stale_commit_is_caught(self):
        r = self.check(self.site, commit="0" * 64)
        self.assertEqual(r.returncode, 1)
        self.assertIn("expected 000000000000", r.stderr)

    def test_install_files_with_a_foreign_key_or_address_are_caught(self):
        c = self.copy_site()
        other = os.path.join(self.d, "refs-other")
        assert sh(REFS, other, self.keys[1][2]).returncode == 0
        shutil.copy(os.path.join(other, f"{APP}.flatpakref"), os.path.join(c, f"{APP}.flatpakref"))
        r = self.check(c)
        self.assertEqual(r.returncode, 1)
        self.assertIn("flatpakref does not carry exactly the project's public key", r.stderr)
        c = self.copy_site()
        p = os.path.join(c, "tsw-forever.flatpakrepo")
        with open(p) as f:
            t = f.read().replace(REPO_URL, "https://other.invalid/repo")
        with open(p, "w") as f:
            f.write(t)
        r = self.check(c)
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not name", r.stderr)

    def test_bad_usage_and_http_outside_the_tests_are_refused(self):
        self.assertEqual(sh(CHECK).returncode, 2)
        r = sh(CHECK, "--site", "http://127.0.0.1:1", "--pub", self.keys[0][2], "--expect-commit", self.commit)
        self.assertEqual(r.returncode, 2)
        self.assertIn("https", r.stderr)
        r = sh(CHECK, "--site", "http://example.invalid", "--pub", self.keys[0][2], "--expect-commit", self.commit, "--allow-loopback-http")
        self.assertEqual(r.returncode, 2)
        self.assertEqual(sh(CHECK, "--site", "https://example.invalid", "--pub", self.keys[0][2], "--expect-commit", "abc").returncode, 2)

    def test_unreachable_site_fails_after_the_rounds(self):
        r = sh(CHECK, "--site", "http://127.0.0.1:9", "--pub", self.keys[0][2], "--expect-commit", self.commit, "--allow-loopback-http", "--retries", "2", "--interval", "0")
        self.assertEqual(r.returncode, 1)
        self.assertIn("FAILED after 2 round(s)", r.stderr)


if __name__ == "__main__":
    unittest.main()
