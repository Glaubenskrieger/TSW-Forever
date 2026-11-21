#!/usr/bin/env python3
"""The mirror of the LIVE Flatpak history (scripts/flatpak_mirror_live.sh) and its use in `scripts/build_flatpak.sh stable-repo`.

The rule under test: a live repository that was GIVEN must be mirrored. A mirror that fails (unreachable, 404, a summary or commit that does not verify, a key that is not trusted) must never
be taken for a first release, because the release would go out silently without its history. A real first release (no live URL, or a live repository that was read with a verified summary and
has no stable branch yet) stays possible and is told apart by its own exit code and message. After a key rotation the previous PUBLIC key has to be named explicitly, and it vouches for the old
history only. GPG verification is never switched off; secret key files are refused.

Two levels: the script itself with real repositories served over a local http server, and the REAL functions of build_flatpak.sh (its dispatcher is cut off, `cmd_check` is a no-op, `flatpak-builder`
is a stub that commits a small application) so that the real exit-code handling of `stable-repo` runs. Throw-away keys only; everything lives in temporary folders that this test removes again.
Skipped where flatpak, gpg or ostree are not installed."""
import os
import re
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

os.environ.setdefault("TSW_TEST_KEY_ALGO", "ed25519")  # speed only: the production algorithm (RSA 4096) is covered by test_flatpak_sign_ci.py and the end-to-end test
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from testtools import requires_tools  # noqa: E402
MIRROR = os.path.join(HERE, "flatpak_mirror_live.sh")
KEYS = os.path.join(HERE, "flatpak_keys.sh")
CHECK = os.path.join(HERE, "check_flatpak_delta_update.sh")
BUILD = os.path.join(HERE, "build_flatpak.sh")
APP = "org.tswlauncher.Launcher"  # the application id build_flatpak.sh stable-repo uses; the test repositories are throw-away ones that only borrow the id
REF = f"app/{APP}/x86_64/stable"

BUILDER_STUB = """#!/bin/bash
# stands in for flatpak-builder: commits a small application into the repository given by --repo=
for a in "$@"; do case "$a" in --repo=*) repo="${a#--repo=}";; esac; done
touch "$TSW_STUB_MARKER"
head -c 300000 /dev/urandom > "$TSW_STUB_APP/files/payload.bin"
flatpak build-export "$repo" "$TSW_STUB_APP" stable >/dev/null
"""


def sh(script, *args, env=None):
    return subprocess.run(["bash", script, *args], capture_output=True, text=True, env=env)


def git_rev(repo, ref, parent=False):
    r = subprocess.run(["ostree", "rev-parse", f"--repo={repo}", ref + ("^" if parent else "")], capture_output=True, text=True)
    return r.stdout.strip() if r.returncode == 0 else None


@requires_tools("flatpak", "gpg", "ostree", "python3", "timeout")
class MirrorLive(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-mirror-")
        cls.k = {}
        for n in "abc":
            home = os.path.join(cls.d, "keys", "gnupg-" + n)
            r = sh(KEYS, "generate-split", home, f"TSW mirror test {n} <{n}@example.invalid>", "1d")
            assert r.returncode == 0, r.stderr
            fpr = r.stdout.strip().splitlines()[-1]
            pub = os.path.join(cls.d, "keys", f"pub-{n}.gpg")
            assert sh(KEYS, "export-public", home, fpr, pub).returncode == 0
            cls.k[n] = (home, fpr, pub)
        cls.apps = {}
        for name in (APP, "org.tswtest.Other"):
            app = os.path.join(cls.d, "app-" + name)
            os.makedirs(os.path.join(app, "files"))
            os.makedirs(os.path.join(app, "export"))
            with open(os.path.join(app, "metadata"), "w") as f:
                f.write(f"[Application]\nname={name}\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
            cls.apps[name] = app
        # the live repositories, served from one folder: signed with A, an unsigned one, one signed with the third key, and one with another application only
        cls.site = os.path.join(cls.d, "site")
        os.makedirs(cls.site)
        for name, signer, app in (("a", "a", APP), ("c", "c", APP), ("other", "a", "org.tswtest.Other"), ("unsigned", None, APP)):
            repo = os.path.join(cls.site, name)
            cls.export(repo, app)
            if signer:
                r = sh(KEYS, "sign-repo", cls.k[signer][0], cls.k[signer][1], repo)
                assert r.returncode == 0, r.stderr
            else:
                subprocess.run(["flatpak", "build-update-repo", repo], capture_output=True, check=True)
        cls.srv = subprocess.Popen(["python3", "-u", "-I", "-m", "http.server", "0", "--bind", "127.0.0.1", "--directory", cls.site], stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
        m = re.search(r"port (\d+)", cls.srv.stdout.readline())
        assert m, "the test server did not start"
        cls.base = f"http://127.0.0.1:{m.group(1)}"
        cls.live_head = git_rev(os.path.join(cls.site, "a"), REF)
        # a secret key file, to be refused
        cls.secret = os.path.join(cls.d, "secret-a.gpg")
        with open(cls.secret, "wb") as f:
            assert subprocess.run(["gpg", "--homedir", cls.k["a"][0], "--batch", "--export-secret-keys", cls.k["a"][1]], stdout=f).returncode == 0
        cls.count = 0

    @classmethod
    def tearDownClass(cls):
        cls.srv.terminate()
        cls.srv.wait()
        cls.srv.stdout.close()
        for home, _f, _p in cls.k.values():
            subprocess.run(["gpgconf", "--homedir", home, "--kill", "all"], capture_output=True)
        shutil.rmtree(cls.d, ignore_errors=True)

    @classmethod
    def export(cls, repo, app):
        with open(os.path.join(cls.apps[app] if app in cls.apps else app, "files", "payload.bin"), "wb") as f:
            f.write(os.urandom(300_000))
        r = subprocess.run(["flatpak", "build-export", repo, cls.apps[app], "stable"], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr

    def new_repo(self):
        type(self).count += 1
        r = os.path.join(self.d, f"work{self.count}")
        subprocess.run(["ostree", "init", f"--repo={r}", "--mode=archive-z2"], check=True, capture_output=True)
        return r

    def mirror(self, repo, url, key, *extras, ref=REF):
        return sh(MIRROR, repo, ref, url, key, *extras)

    # ---- the script ----------------------------------------------------------------------------------------------------------------------------------------------------------------------

    def test_without_a_live_url_it_is_a_first_release_and_says_so(self):
        r = self.new_repo()
        res = self.mirror(r, "", self.k["a"][2])
        self.assertEqual(res.returncode, 3, res.stdout + res.stderr)
        self.assertIn("MIRROR: first release (no live repository configured)", res.stdout)
        self.assertIsNone(git_rev(r, REF))

    def test_a_live_repository_is_mirrored_and_its_history_is_taken_over(self):
        r = self.new_repo()
        res = self.mirror(r, f"{self.base}/a", self.k["a"][2])
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("MIRROR: mirrored", res.stdout)
        self.assertEqual(git_rev(r, REF), self.live_head)
        self.assertNotIn("WARNING", res.stdout, "no extra key was named")
        # the next release is made on top of it: the live head becomes its parent (so that deltas live -> new exist)
        self.export(r, APP)
        self.assertEqual(git_rev(r, REF, parent=True), self.live_head)

    def test_a_live_repository_without_the_branch_is_a_first_release_of_that_branch_not_a_failure(self):
        r = self.new_repo()
        res = self.mirror(r, f"{self.base}/other", self.k["a"][2])
        self.assertEqual(res.returncode, 3, res.stdout + res.stderr)
        self.assertIn("MIRROR: first release of this branch", res.stdout)
        self.assertIn("its summary verified", res.stdout)
        self.assertIsNone(git_rev(r, REF))

    def test_a_key_rotation_fails_until_the_previous_key_is_named_and_then_takes_the_history_over(self):
        pub_a, pub_b = self.k["a"][2], self.k["b"][2]
        r = self.new_repo()
        res = self.mirror(r, f"{self.base}/a", pub_b)  # the live history is signed with A, this release has key B
        self.assertEqual(res.returncode, 19, res.stdout + res.stderr)
        self.assertIn("MIRROR: FAILED", res.stderr)
        self.assertIn("PREVIOUS key", res.stderr)
        self.assertIn("FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS", res.stderr)
        self.assertIsNone(git_rev(r, REF), "nothing was taken over")
        r = self.new_repo()
        res = self.mirror(r, f"{self.base}/a", pub_b, pub_a)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertRegex(res.stdout, r"MIRROR: WARNING: also trusting, for the OLD history only \(explicit previous key\): .*pub-a\.gpg [0-9A-F]{40}")
        self.assertEqual(git_rev(r, REF), self.live_head)

    def test_an_extra_old_key_never_confirms_a_new_release_commit(self):
        pub_a, pub_b = self.k["a"][2], self.k["b"][2]
        r = self.new_repo()
        self.assertEqual(self.mirror(r, f"{self.base}/a", pub_b, pub_a).returncode, 0)
        self.export(r, APP)  # the new release, on top of the mirrored history
        # signed with the OLD key only: it must not be valid for the release key B, neither for verify-repo nor for the delta check
        self.assertEqual(sh(KEYS, "sign-repo", self.k["a"][0], self.k["a"][1], r).returncode, 0)
        self.assertEqual(sh(KEYS, "verify-repo", pub_b, r).returncode, 1, "verify-repo with the release key refuses a commit that only the old key signed")
        res = sh(CHECK, "--require-delta", "--extra-key", pub_a, r, pub_b)
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("does not verify with the given key alone", res.stdout)
        # signed with the release key B: valid
        r2 = self.new_repo()
        self.assertEqual(self.mirror(r2, f"{self.base}/a", pub_b, pub_a).returncode, 0)
        self.export(r2, APP)
        self.assertEqual(sh(KEYS, "sign-repo", self.k["b"][0], self.k["b"][1], r2).returncode, 0)
        self.assertEqual(sh(KEYS, "verify-repo", pub_b, r2).returncode, 0)
        res = sh(CHECK, "--require-delta", "--extra-key", pub_a, r2, pub_b)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)

    def test_missing_secret_and_unusable_key_files_are_refused(self):
        r = self.new_repo()
        url = f"{self.base}/a"
        res = self.mirror(r, url, os.path.join(self.d, "missing.gpg"))
        self.assertEqual((res.returncode, "empty or missing" in res.stderr), (18, True), res.stdout + res.stderr)
        res = self.mirror(r, url, self.k["a"][2], os.path.join(self.d, "missing.gpg"))
        self.assertEqual((res.returncode, "empty or missing" in res.stderr), (18, True), res.stdout + res.stderr)
        for args in ((self.secret,), (self.k["b"][2], self.secret)):
            res = self.mirror(r, url, *args)
            self.assertEqual((res.returncode, "SECRET" in res.stderr), (18, True), res.stdout + res.stderr)
        empty = os.path.join(self.d, "empty.gpg")
        open(empty, "w").close()
        self.assertEqual(self.mirror(r, url, empty).returncode, 18)
        self.assertIsNone(git_rev(r, REF))

    def test_an_invalid_or_missing_signature_is_a_failure_not_a_first_release(self):
        pub_a = self.k["a"][2]
        for name, why in (("unsigned", "no signature at all"), ("c", "signed with a key that is not trusted")):
            r = self.new_repo()
            res = self.mirror(r, f"{self.base}/{name}", pub_a)
            self.assertEqual(res.returncode, 19, f"{why}: " + res.stdout + res.stderr)
            self.assertIn("MIRROR: FAILED", res.stderr)
            self.assertIsNone(git_rev(r, REF), why)

    def test_network_errors_are_failures_not_first_releases(self):
        pub_a = self.k["a"][2]
        for url, why in (("http://127.0.0.1:1/repo", "nothing listens"), (f"{self.base}/no-such-repo", "404"), (f"{self.base}/", "not a repository")):
            r = self.new_repo()
            res = self.mirror(r, url, pub_a)
            self.assertEqual(res.returncode, 19, f"{why}: " + res.stdout + res.stderr)
            self.assertIn("For a real FIRST release do not give a live URL", res.stderr)
            self.assertIsNone(git_rev(r, REF), why)

    def test_bad_calls_are_refused(self):
        r = self.new_repo()
        pub = self.k["a"][2]
        self.assertEqual(sh(MIRROR).returncode, 2)
        self.assertEqual(sh(MIRROR, r, REF, "").returncode, 2)
        self.assertEqual(self.mirror(os.path.join(self.d, "no-repo"), "", pub).returncode, 2)
        self.assertEqual(self.mirror(r, "ftp://example.invalid/repo", pub).returncode, 2)
        self.assertEqual(self.mirror(r, f"{self.base}/a", pub, ref="runtime/x/y/z").returncode, 2)

    def test_gpg_verification_is_never_switched_off(self):
        for path in (MIRROR, BUILD):
            src = open(path, encoding="utf-8").read()
            for forbidden in ("gpg-verify=false", "--no-gpg-verify", "gpg-verify-summary=false"):
                self.assertNotIn(forbidden, src, path)
        self.assertIn("--set=gpg-verify-summary=true", open(MIRROR, encoding="utf-8").read(), "the summary is verified as well")

    # ---- the real stable-repo of build_flatpak.sh ------------------------------------------------------------------------------------------------------------------------------------------

    def stable_repo(self, url=None, key=None, extra=None):
        """Run the REAL `cmd_stable_repo` of build_flatpak.sh: its dispatcher is cut off, cmd_check is a no-op and flatpak-builder is the stub that commits the small application."""
        type(self).count += 1
        root = os.path.join(self.d, f"tree{self.count}")
        os.makedirs(os.path.join(root, "scripts"))
        os.makedirs(os.path.join(root, "build", "flatpak", "src", "vendor"))
        os.makedirs(os.path.join(root, "bin"))
        shutil.copy(MIRROR, os.path.join(root, "scripts", "flatpak_mirror_live.sh"))
        src = open(BUILD, encoding="utf-8").read()
        marker = '\ncase "${1:-}" in\n  fetch)'
        self.assertIn(marker, src, "the dispatcher of build_flatpak.sh moved: adapt this test")
        with open(os.path.join(root, "scripts", "build_flatpak.sh"), "w", encoding="utf-8") as f:
            f.write(src[: src.index(marker)] + "\ncmd_check() { :; }\ncmd_stable_repo\n")
        stub = os.path.join(root, "bin", "flatpak-builder")
        with open(stub, "w") as f:
            f.write(BUILDER_STUB)
        os.chmod(stub, os.stat(stub).st_mode | stat.S_IXUSR)
        env = dict(os.environ, PATH=os.path.join(root, "bin") + os.pathsep + os.environ["PATH"], TSW_STUB_APP=self.apps[APP], TSW_STUB_MARKER=os.path.join(root, "builder-ran"))
        for v in ("FLATPAK_LIVE_REPO_URL", "FLATPAK_STABLE_KEY", "FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS"):
            env.pop(v, None)
        if url is not None:
            env["FLATPAK_LIVE_REPO_URL"] = url
        if key is not None:
            env["FLATPAK_STABLE_KEY"] = key
        if extra is not None:
            env["FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS"] = extra
        res = subprocess.run(["bash", os.path.join(root, "scripts", "build_flatpak.sh")], capture_output=True, text=True, env=env, cwd=root)
        repo = os.path.join(root, "build", "flatpak", "repo")
        return res, repo, os.path.exists(env["TSW_STUB_MARKER"])

    def test_stable_repo_without_a_live_url_builds_a_first_release_without_parent(self):
        res, repo, built = self.stable_repo()
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(built)
        self.assertIn("first release: no live repository is configured", res.stderr)
        self.assertIsNone(git_rev(repo, REF, parent=True))

    def test_stable_repo_mirrors_the_live_history_and_the_new_commit_gets_it_as_its_parent(self):
        res, repo, built = self.stable_repo(url=f"{self.base}/a", key=self.k["a"][2])
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(built)
        self.assertEqual(git_rev(repo, REF, parent=True), self.live_head)

    def test_stable_repo_builds_nothing_when_a_given_live_repository_cannot_be_mirrored(self):
        cases = (
            ("unreachable", dict(url="http://127.0.0.1:1/repo", key=self.k["a"][2])),
            ("404", dict(url=f"{self.base}/no-such-repo", key=self.k["a"][2])),
            ("unsigned live repository", dict(url=f"{self.base}/unsigned", key=self.k["a"][2])),
            ("live history signed with a key that is not trusted (rotation without the previous key)", dict(url=f"{self.base}/a", key=self.k["b"][2])),
        )
        for why, kw in cases:
            res, repo, built = self.stable_repo(**kw)
            self.assertEqual(res.returncode, 19, f"{why}: " + res.stdout + res.stderr)
            self.assertFalse(built, f"{why}: no release is built without the history")
            self.assertIn("no release is built", res.stderr, why)
            self.assertNotIn("first release", res.stderr.replace("FIRST release", ""), f"{why}: a failed mirror is never called a first release")

    def test_stable_repo_treats_a_verified_live_repository_without_the_branch_as_a_first_release_of_it(self):
        res, repo, built = self.stable_repo(url=f"{self.base}/other", key=self.k["a"][2])
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(built)
        self.assertIn("MIRROR: first release of this branch", res.stdout)
        self.assertIn("continuing without history", res.stderr)
        self.assertIsNone(git_rev(repo, REF, parent=True))

    def test_stable_repo_after_a_key_rotation_needs_the_previous_key_named_explicitly(self):
        pub_a, pub_b = self.k["a"][2], self.k["b"][2]
        res, repo, built = self.stable_repo(url=f"{self.base}/a", key=pub_b)
        self.assertEqual((res.returncode, built), (19, False), res.stdout + res.stderr)
        res, repo, built = self.stable_repo(url=f"{self.base}/a", key=pub_b, extra=pub_a)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(built)
        self.assertEqual(git_rev(repo, REF, parent=True), self.live_head)
        self.assertIn("also trusting, for the OLD history only", res.stdout)
        res, repo, built = self.stable_repo(url=f"{self.base}/a", key=pub_b, extra=self.secret)
        self.assertEqual((res.returncode, built), (18, False), "a secret key file is refused: " + res.stdout + res.stderr)

    def test_stable_repo_refuses_a_missing_public_key_before_anything_is_built(self):
        res, repo, built = self.stable_repo(url=f"{self.base}/a", key=os.path.join(self.d, "missing.gpg"))
        self.assertEqual((res.returncode, built), (18, False), res.stdout + res.stderr)


if __name__ == "__main__":
    unittest.main()
