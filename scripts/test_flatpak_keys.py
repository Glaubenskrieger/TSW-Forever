#!/usr/bin/env python3
"""scripts/flatpak_keys.sh with THROW-AWAY keys in a temporary keyring outside the repository: offline signing and the verification a user's Flatpak performs.
Skipped where flatpak, gpg or ostree are not installed. No production key is involved at any point."""
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
SCRIPT = os.path.join(HERE, "flatpak_keys.sh")


def run(*args, **kw):
    return subprocess.run(["bash", SCRIPT, *args], capture_output=True, text=True, **kw)


@requires_tools("flatpak", "gpg", "ostree")
class Keys(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-keys-")
        cls.keys = []
        for n in ("a", "b"):
            home = os.path.join(cls.d, "gnupg-" + n)
            r = run("generate-split", home, f"TSW throw-away {n} <{n}@example.invalid>", "1d")
            assert r.returncode == 0, r.stderr
            fpr = r.stdout.strip().splitlines()[-1]
            pub = os.path.join(cls.d, f"pub-{n}.gpg")
            assert run("export-public", home, fpr, pub).returncode == 0
            cls.keys.append((home, fpr, pub))
        app = os.path.join(cls.d, "app")
        os.makedirs(os.path.join(app, "files"))
        os.makedirs(os.path.join(app, "export"))
        with open(os.path.join(app, "metadata"), "w") as f:
            f.write("[Application]\nname=org.tswtest.Keys\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
        cls.repo = os.path.join(cls.d, "repo")
        r = subprocess.run(["flatpak", "build-export", cls.repo, app, "stable"], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.d, ignore_errors=True)

    def test_an_unsigned_repository_does_not_verify_and_a_signed_one_only_with_its_own_key(self):
        (home_a, fpr_a, pub_a), (_, _, pub_b) = self.keys
        copy = os.path.join(self.d, "copy")
        shutil.copytree(self.repo, copy)
        self.assertEqual(run("verify-repo", pub_a, copy).returncode, 1, "unsigned")
        r = run("sign-repo", home_a, fpr_a, copy)
        self.assertEqual(r.returncode, 0, r.stderr)
        r = run("verify-repo", pub_a, copy)
        self.assertEqual((r.returncode, "verified: 1 ref" in r.stdout), (0, True), r.stderr)
        self.assertEqual(run("verify-repo", pub_b, copy).returncode, 1, "another key must not verify it")

    def test_the_public_export_is_public_and_the_keyring_must_stay_outside_the_repository(self):
        pub = self.keys[0][2]
        out = subprocess.run(["gpg", "--list-packets", pub], capture_output=True, text=True).stdout
        self.assertIn(":public key packet:", out)
        self.assertNotIn(":secret", out)
        inside = os.path.join(HERE, "..", "build", "keyring-must-not-be-created")
        r = run("generate-split", inside, "x <x@example.invalid>")
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertFalse(os.path.exists(inside))
        # an existing keyring is never reused or overwritten
        self.assertEqual(run("generate-split", self.keys[0][0], "x <x@example.invalid>").returncode, 4)

    def test_bad_usage_is_refused(self):
        self.assertEqual(run().returncode, 2)
        self.assertNotEqual(run("verify-repo", os.path.join(self.d, "missing.gpg"), self.repo).returncode, 0)
        self.assertNotEqual(run("verify-repo", self.keys[0][2], os.path.join(self.d, "no-repo")).returncode, 0)


@requires_tools("flatpak", "gpg", "ostree")
class SplitKeys(unittest.TestCase):
    """The PRODUCTION model with THROW-AWAY keys: a certify-only primary key that never leaves the offline keyring and a signing subkey that is the only secret CI gets."""

    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-split-")
        cls.home = os.path.join(cls.d, "gnupg")
        r = run("generate-split", cls.home, "TSW split throw-away <split@example.invalid>")
        assert r.returncode == 0, r.stderr
        cls.fpr = r.stdout.strip().splitlines()[-1]
        cls.pub = os.path.join(cls.d, "pub.gpg")
        assert run("export-public", cls.home, cls.fpr, cls.pub).returncode == 0
        cls.ci = os.path.join(cls.d, "ci.b64")
        r = run("export-ci-subkey", cls.home, cls.fpr, cls.ci)
        assert r.returncode == 0, r.stderr
        app = os.path.join(cls.d, "app")
        os.makedirs(os.path.join(app, "files"))
        os.makedirs(os.path.join(app, "export"))
        with open(os.path.join(app, "metadata"), "w") as f:
            f.write("[Application]\nname=org.tswtest.Keys\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
        cls.repo = os.path.join(cls.d, "repo")
        r = subprocess.run(["flatpak", "build-export", cls.repo, app, "stable"], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.d, ignore_errors=True)

    def gpg(self, home, *args, stdin=None):
        env = dict(os.environ, GNUPGHOME=home)
        return subprocess.run(["gpg", "--batch", *args], capture_output=True, text=True, env=env, input=stdin)

    def test_the_primary_key_can_only_certify_and_a_separate_subkey_signs(self):
        out = subprocess.run(["gpg", "--show-keys", "--with-colons", self.pub], capture_output=True, text=True).stdout
        pub = [l.split(":") for l in out.splitlines() if l.startswith("pub:")][0]
        sub = [l.split(":") for l in out.splitlines() if l.startswith("sub:")]
        own = "".join(ch for ch in pub[11] if ch.islower())  # lower case = this key's own capabilities, upper case = the combined ones of the subkeys
        self.assertEqual(own, "c", "the primary key only certifies")
        self.assertEqual(len(sub), 1)
        self.assertEqual("".join(ch for ch in sub[0][11] if ch.islower()), "s")
        fp = run("fingerprints", self.pub).stdout.split()
        self.assertEqual((fp[0], fp[1]), ("primary", self.fpr))
        self.assertEqual(fp[2], "subkey")
        self.assertNotEqual(fp[3], self.fpr)
        self.assertEqual(pub[6], "", "default: the primary key does not expire")
        self.assertEqual(sub[0][6], "", "default: the signing subkey does not expire")

    def test_the_ci_export_is_one_line_holds_no_primary_secret_and_the_public_export_no_secret_at_all(self):
        text = open(self.ci).read()
        self.assertNotIn("\n", text)
        self.assertNotIn("PRIVATE KEY", text, "base64: no armor header in the clear")
        import base64
        armored = base64.b64decode(text).decode()
        self.assertIn("BEGIN PGP PRIVATE KEY BLOCK", armored)
        chk = tempfile.mkdtemp(prefix="tsw-chk-")
        self.addCleanup(shutil.rmtree, chk, True)
        os.chmod(chk, 0o700)
        self.assertEqual(self.gpg(chk, "--import", stdin=armored).returncode, 0)
        listing = self.gpg(chk, "--list-secret-keys").stdout
        self.assertIn("sec#", listing, "the primary secret key must be a stub")
        self.assertIn("ssb", listing)
        self.assertNotIn(":secret", subprocess.run(["gpg", "--list-packets", self.pub], capture_output=True, text=True).stdout)

    def test_a_repository_signed_with_the_ci_subkey_only_verifies_against_the_public_key_and_the_primary_secret_is_not_needed(self):
        ci_home = tempfile.mkdtemp(prefix="tsw-ci-")
        self.addCleanup(shutil.rmtree, ci_home, True)
        os.chmod(ci_home, 0o700)
        import base64
        armored = base64.b64decode(open(self.ci).read()).decode()
        self.assertEqual(self.gpg(ci_home, "--import", stdin=armored).returncode, 0)
        copy = os.path.join(self.d, "copy")
        shutil.copytree(self.repo, copy)
        self.assertEqual(run("verify-repo", self.pub, copy).returncode, 1, "unsigned")
        r = run("sign-repo", ci_home, self.fpr, copy)
        self.assertEqual(r.returncode, 0, r.stderr)
        r = run("verify-repo", self.pub, copy)
        self.assertEqual((r.returncode, "verified: 1 ref" in r.stdout), (0, True), r.stderr)

    def test_the_ci_export_refuses_a_target_inside_the_repository_and_a_wrong_key(self):
        inside = os.path.join(HERE, "..", "build", "ci-key-must-not-be-created")
        r = run("export-ci-subkey", self.home, self.fpr, inside)
        self.assertEqual(r.returncode, 3, r.stderr)
        self.assertFalse(os.path.exists(inside))
        r = run("export-ci-subkey", self.home, "0" * 40, os.path.join(self.d, "x.b64"))
        self.assertNotEqual(r.returncode, 0)
        self.assertFalse(os.path.exists(os.path.join(self.d, "x.b64")))

    def test_check_pin_accepts_only_the_pinned_primary_fingerprint_and_subkey_id_and_only_public_parts(self):
        fp = run("fingerprints", self.pub).stdout.split()
        subid = fp[3][-16:]
        conf = os.path.join(self.d, "project.conf")

        def pin(f, s):
            with open(conf, "w") as fh:
                fh.write(f"FLATPAK_SIGNING_FINGERPRINT={f}\nFLATPAK_SIGNING_SUBKEY_ID={s}\n")

        pin(self.fpr, subid)
        r = run("check-pin", self.pub, conf)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("public parts only", r.stdout)
        pin(self.fpr.lower(), subid.lower())
        self.assertEqual(run("check-pin", self.pub, conf).returncode, 0, "case does not matter")
        pin("0" * 40, subid)
        self.assertEqual(run("check-pin", self.pub, conf).returncode, 1)
        pin(self.fpr, "0" * 16)
        r = run("check-pin", self.pub, conf)
        self.assertEqual(r.returncode, 1)
        self.assertIn("pinned id", r.stderr)
        for bad in (("", ""), (self.fpr[:39], subid), (self.fpr, subid[:15])):
            pin(*bad)
            self.assertEqual(run("check-pin", self.pub, conf).returncode, 1, bad)
        pin(self.fpr, subid)
        secret = os.path.join(self.d, "secret.gpg")
        import base64
        with open(secret, "wb") as fh:
            fh.write(base64.b64decode(open(self.ci).read()))
        r = run("check-pin", secret, conf)
        self.assertEqual(r.returncode, 1)
        self.assertIn("SECRET", r.stderr)
        self.assertEqual(run("check-pin", os.path.join(self.d, "missing.gpg"), conf).returncode, 1)
        # the real project.conf pins the project's keys: the format is checked here, the key file itself by the pipeline
        real = os.path.join(HERE, "..", "packaging", "project.conf")
        with open(real) as fh:
            text = fh.read()
        self.assertRegex(text, r"(?m)^FLATPAK_SIGNING_FINGERPRINT=FAF3884F3E41D5C63FE656DC86075D3D53A6484B$")
        self.assertRegex(text, r"(?m)^FLATPAK_SIGNING_SUBKEY_ID=43F56AAA62FBF7DA$")

    def test_a_new_keyring_is_never_an_existing_one_and_bad_usage_is_refused(self):
        self.assertEqual(run("generate-split", self.home, "x <x@example.invalid>").returncode, 4)
        self.assertNotEqual(run("export-ci-subkey").returncode, 0)


if __name__ == "__main__":
    unittest.main()
