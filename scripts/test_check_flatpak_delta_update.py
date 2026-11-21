#!/usr/bin/env python3
"""scripts/check_flatpak_delta_update.sh with THROW-AWAY keys and tiny repositories built with the real tools (flatpak build-export, flatpak_keys.sh sign-repo): the update over the
SIGNED static delta path must pass, and every way it can be broken must fail: a delta generated BEFORE the commit was signed (the failure that stopped a test update at 0 %,
while `flatpak_keys.sh verify-repo` stayed green), an unsigned repository, another key, and a release without deltas. Skipped where flatpak, gpg or ostree are not installed.
No production key is involved at any point; everything lives in temporary folders that this test removes again."""
import os
import shutil
import subprocess
import sys
import tempfile
import unittest

os.environ.setdefault("TSW_TEST_KEY_ALGO", "ed25519")  # speed only: the production algorithm (RSA 4096) is covered by test_flatpak_sign_ci.py and the end-to-end test
HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from testtools import requires_tools  # noqa: E402
KEYS = os.path.join(HERE, "flatpak_keys.sh")
CHECK = os.path.join(HERE, "check_flatpak_delta_update.sh")


def sh(script, *args, **kw):
    return subprocess.run(["bash", script, *args], capture_output=True, text=True, **kw)


def check(*args):
    return sh(CHECK, *args)


def tree_state(root):
    """Every file of a folder with size and time: a read-only check must leave this unchanged."""
    out = []
    for base, _dirs, files in os.walk(root):
        for f in sorted(files):
            p = os.path.join(base, f)
            st = os.lstat(p)
            out.append((os.path.relpath(p, root), st.st_size, st.st_mtime_ns))
    return sorted(out)


@requires_tools("flatpak", "gpg", "ostree", "python3")
class DeltaUpdate(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-delta-")
        cls.homes = []
        cls.keys = []
        for n in ("a", "b"):
            home = os.path.join(cls.d, "gnupg-" + n)
            r = sh(KEYS, "generate-split", home, f"TSW delta test {n} <{n}@example.invalid>", "1d")
            assert r.returncode == 0, r.stderr
            fpr = r.stdout.strip().splitlines()[-1]
            pub = os.path.join(cls.d, f"pub-{n}.gpg")
            assert sh(KEYS, "export-public", home, fpr, pub).returncode == 0
            cls.homes.append(home)
            cls.keys.append((home, fpr, pub))
        # a tiny application; every release is an export of it with new content (a real delta between two releases needs a changed file)
        cls.app = os.path.join(cls.d, "app")
        os.makedirs(os.path.join(cls.app, "files"))
        os.makedirs(os.path.join(cls.app, "export"))
        with open(os.path.join(cls.app, "metadata"), "w") as f:
            f.write("[Application]\nname=org.tswtest.Delta\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
        cls.ref = "app/org.tswtest.Delta/x86_64/stable"
        cls.cache = {}

    @classmethod
    def export(cls, repo):
        with open(os.path.join(cls.app, "files", "payload.bin"), "wb") as f:
            f.write(os.urandom(300_000))
        r = subprocess.run(["flatpak", "build-export", repo, cls.app, "stable"], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr

    @classmethod
    def tearDownClass(cls):
        for home in cls.homes:
            subprocess.run(["gpgconf", "--homedir", home, "--kill", "all"], capture_output=True)
        shutil.rmtree(cls.d, ignore_errors=True)

    def repo(self, kind):
        """A repository with two releases, the second finished the way `kind` says (built once per kind). Like in a real history the FIRST release was signed in its own run
        (`flatpak build-sign` only signs the newest commit of a branch)."""
        if kind in self.cache:
            return self.cache[kind]
        home, fpr, _pub = self.keys[0]
        r = os.path.join(self.d, "repo-" + kind)
        self.export(r)  # release 1
        if kind in ("good", "stale"):
            self.assertEqual(sh(KEYS, "sign-repo", home, fpr, r).returncode, 0)
        self.export(r)  # release 2 (its parent is release 1)
        if kind == "good":  # the release order: sign the commit, THEN generate the deltas (flatpak_keys.sh sign-repo does both, in this order)
            self.assertEqual(sh(KEYS, "sign-repo", home, fpr, r).returncode, 0)
        elif kind == "stale":  # the mistake: deltas first, signing afterwards
            self.assertEqual(subprocess.run(["flatpak", "build-update-repo", "--generate-static-deltas", r], capture_output=True).returncode, 0)
            self.assertEqual(sh(KEYS, "sign-repo", home, fpr, r).returncode, 0)
        elif kind == "unsigned":
            self.assertEqual(subprocess.run(["flatpak", "build-update-repo", "--generate-static-deltas", r], capture_output=True).returncode, 0)
        elif kind == "nodelta":  # a signed commit and a signed summary, but no static deltas at all
            self.assertEqual(subprocess.run(["flatpak", "build-sign", f"--gpg-sign={fpr}", f"--gpg-homedir={home}", r, "org.tswtest.Delta", "stable"], capture_output=True).returncode, 0)
            self.assertEqual(subprocess.run(["flatpak", "build-update-repo", f"--gpg-sign={fpr}", f"--gpg-homedir={home}", r], capture_output=True).returncode, 0)
        else:
            raise AssertionError(kind)
        self.cache[kind] = r
        return r

    def rotation_repo(self):
        """Release 1 signed with key A (as a mirrored history), release 2 signed with key B only (sign-repo makes its deltas after the signing)."""
        if "rotation" in self.cache:
            return self.cache["rotation"]
        (home_a, fpr_a, _), (home_b, fpr_b, _) = self.keys
        r = os.path.join(self.d, "repo-rotation")
        self.export(r)
        self.assertEqual(sh(KEYS, "sign-repo", home_a, fpr_a, r).returncode, 0)
        self.export(r)
        self.assertEqual(sh(KEYS, "sign-repo", home_b, fpr_b, r).returncode, 0)
        self.cache["rotation"] = r
        return r

    def commit_from_repo(self, remove_unsigned_deltas):
        """The way the throw-away test repositories make their second release: `flatpak build-commit-from` copies release 1 (signed) into a new commit. It generates the delta
        "scratch -> new commit" at once, while the new commit is still unsigned; sign-repo then signs the commit but does not regenerate a delta that exists."""
        key = ("commit-from", remove_unsigned_deltas)
        if key in self.cache:
            return self.cache[key]
        home, fpr, _pub = self.keys[0]
        src = os.path.join(self.d, "src-release1")
        if not os.path.isdir(src):
            self.export(src)
            self.assertEqual(sh(KEYS, "sign-repo", home, fpr, src).returncode, 0)
        r = os.path.join(self.d, f"repo-commit-from-{int(remove_unsigned_deltas)}")
        shutil.copytree(src, r)
        res = subprocess.run(["flatpak", "build-commit-from", f"--src-repo={src}", f"--src-ref={self.ref}", "--force", "--timestamp=NOW", r, self.ref], capture_output=True, text=True)
        self.assertEqual(res.returncode, 0, res.stderr)
        if remove_unsigned_deltas:
            head = subprocess.run(["ostree", "rev-parse", f"--repo={r}", self.ref], capture_output=True, text=True).stdout.strip()
            names = subprocess.run(["ostree", "static-delta", "list", f"--repo={r}"], capture_output=True, text=True).stdout.split()
            for name in names:
                if name == head or name.endswith("-" + head):
                    self.assertEqual(subprocess.run(["ostree", "static-delta", "delete", f"--repo={r}", name], capture_output=True).returncode, 0)
        self.assertEqual(sh(KEYS, "sign-repo", home, fpr, r).returncode, 0)
        self.cache[key] = r
        return r

    def test_the_release_order_passes_over_every_signed_delta_and_changes_nothing(self):
        r = self.repo("good")
        before = tree_state(r)
        res = check("--require-delta", r, self.keys[0][2])
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("all checks passed", res.stdout)
        self.assertIn("scratch ->", res.stdout, "the delta from scratch is tested")
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 2, "from scratch and from the first commit: " + res.stdout)
        self.assertEqual(tree_state(r), before, "the check only reads the repository")

    def test_a_delta_generated_before_the_signing_fails_although_verify_repo_is_green(self):
        r = self.repo("stale")
        pub = self.keys[0][2]
        verify = sh(KEYS, "verify-repo", pub, r)
        self.assertEqual((verify.returncode, "verified: 1 ref" in verify.stdout), (0, True), "verify-repo does not see the problem: " + verify.stdout + verify.stderr)
        res = check("--require-delta", r, pub)
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("no signatures found", res.stdout)
        self.assertIn("FAILED", res.stdout)

    def test_build_commit_from_leaves_an_unsigned_scratch_delta_that_sign_repo_does_not_replace(self):
        res = check("--require-delta", self.commit_from_repo(False), self.keys[0][2])
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertRegex(res.stdout, r"FAIL: .*scratch -> .*no signatures found")
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 1, "only the delta from the previous release is made after the signing: " + res.stdout)

    def test_removing_the_deltas_of_the_new_commit_before_sign_repo_makes_every_delta_signed(self):
        res = check("--require-delta", self.commit_from_repo(True), self.keys[0][2])
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 2, res.stdout)

    def test_after_a_key_rotation_the_older_commit_is_not_trusted_until_the_previous_key_is_named(self):
        r, pub_a, pub_b = self.rotation_repo(), self.keys[0][2], self.keys[1][2]
        res = check("--require-delta", r, pub_b)
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("the older commit could not be fetched and verified first", res.stdout)
        self.assertNotIn("WARNING", res.stdout, "nothing is trusted silently")
        res = check("--require-delta", "--extra-key", pub_a, r, pub_b)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 2, res.stdout)
        self.assertRegex(res.stdout, r"WARNING: also trusting, for OLDER commits only \(explicit --extra-key\): .*pub-a\.gpg")
        self.assertIn("verifies with the given key alone", res.stdout)

    def test_an_extra_key_never_vouches_for_the_new_state(self):
        # key A is given as THE key and B only as an extra: the newest commit and the summary are signed with B, so they must fail although B is trusted for history
        r, pub_a, pub_b = self.rotation_repo(), self.keys[0][2], self.keys[1][2]
        res = check("--require-delta", "--extra-key", pub_b, r, pub_a)
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("the summary could not be verified with the given public key", res.stdout)
        self.assertIn("does not verify with the given key alone", res.stdout)

    def test_key_files_must_be_public_keys_that_exist(self):
        r, pub_a, pub_b = self.rotation_repo(), self.keys[0][2], self.keys[1][2]
        secret = os.path.join(self.d, "secret-a.gpg")
        with open(secret, "wb") as f:
            self.assertEqual(subprocess.run(["gpg", "--homedir", self.keys[0][0], "--batch", "--export-secret-keys", self.keys[0][1]], stdout=f).returncode, 0)
        res = check("--extra-key", secret, r, pub_b)
        self.assertEqual((res.returncode, "SECRET" in res.stderr), (2, True), res.stdout + res.stderr)
        res = check(r, secret)
        self.assertEqual((res.returncode, "SECRET" in res.stderr), (2, True), "the given key as well: " + res.stdout + res.stderr)
        self.assertEqual(check("--extra-key", os.path.join(self.d, "missing.gpg"), r, pub_b).returncode, 2)
        self.assertEqual(check(r, pub_b, "--extra-key").returncode, 2, "--extra-key needs a file")

    def test_an_unsigned_repository_fails(self):
        res = check("--require-delta", self.repo("unsigned"), self.keys[0][2])
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("FAIL: ", res.stdout)

    def test_another_key_fails(self):
        res = check("--require-delta", self.repo("good"), self.keys[1][2])
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("public key not found", res.stdout)

    def test_a_release_without_deltas_fails_when_deltas_are_required_and_is_only_noted_otherwise(self):
        r = self.repo("nodelta")
        pub = self.keys[0][2]
        res = check("--require-delta", r, pub)
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("no static delta targets", res.stdout)
        res = check(r, pub)
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertIn("the delta path was not tested", res.stdout)

    def test_a_ref_that_does_not_exist_fails(self):
        res = check("--require-delta", self.repo("good"), self.keys[0][2], "app/org.tswtest.Missing/x86_64/stable")
        self.assertEqual(res.returncode, 1, res.stdout + res.stderr)
        self.assertIn("no such ref", res.stdout)

    def test_bad_usage_is_refused_with_exit_2(self):
        pub = self.keys[0][2]
        self.assertEqual(check().returncode, 2)
        self.assertEqual(check("--bogus", self.repo("good"), pub).returncode, 2)
        self.assertEqual(check(self.repo("good"), os.path.join(self.d, "missing.gpg")).returncode, 2)
        self.assertEqual(check(os.path.join(self.d, "no-repo"), pub).returncode, 2)
        self.assertEqual(check("--help").returncode, 0)


if __name__ == "__main__":
    unittest.main()
