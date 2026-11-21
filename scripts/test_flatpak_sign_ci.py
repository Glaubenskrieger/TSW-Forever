#!/usr/bin/env python3
"""scripts/flatpak_sign_ci.sh (the signing step of the release pipeline) with THROW-AWAY keys only. The subkey and its passphrase come from the environment like in the protected GitHub
environment; the committed public key, the primary fingerprint and the subkey id must fit; the primary secret key must be absent; nothing of the key or the passphrase may appear in the output,
in the repository or in the left-over temporary folder. No production key is involved at any point: the production keys exist outside this repository and are never created by it."""
import base64
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)
from testtools import requires_tools  # noqa: E402
KEYS = os.path.join(HERE, "flatpak_keys.sh")
SIGN = os.path.join(HERE, "flatpak_sign_ci.sh")
PW = "Correct Horse 'Battery' Staple 42"


def keys(*args, env=None):
    return subprocess.run(["bash", KEYS, *args], capture_output=True, text=True, env=dict(os.environ, **(env or {})))


@requires_tools("flatpak", "gpg", "ostree", "base64", "python3")
class SignCi(unittest.TestCase):
    @classmethod
    def mk(cls, name, algo="rsa4096", pw=""):  # RSA 4096 is the production algorithm
        home = os.path.join(cls.d, "gnupg-" + name)
        env = {"TSW_TEST_KEY_ALGO": algo, "TSW_TEST_KEY_PASSPHRASE": pw}
        r = keys("generate-split", home, f"TSW signci throw-away {name} <{name}@example.invalid>", env=env)
        assert r.returncode == 0, r.stderr
        fpr = r.stdout.strip().splitlines()[-1]
        pub = os.path.join(cls.d, f"pub-{name}.gpg")
        assert keys("export-public", home, fpr, pub).returncode == 0
        ci = os.path.join(cls.d, f"ci-{name}.b64")
        r = keys("export-ci-subkey", home, fpr, ci, env=env)
        assert r.returncode == 0, r.stderr
        fps = keys("fingerprints", pub).stdout.split()
        subfpr = fps[fps.index("subkey") + 1]
        with open(ci) as f:
            return {"home": home, "fpr": fpr, "subid": subfpr[-16:], "pub": pub, "ci": f.read().strip(), "pw": pw}

    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-signci-")
        cls.a = cls.mk("a")
        cls.b = cls.mk("b", algo="ed25519", pw=PW)
        cls.c = cls.mk("c", algo="rsa4096", pw=PW)
        cls.o = cls.mk("o", algo="ed25519")
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

    def fresh_repo(self):
        copy = tempfile.mkdtemp(prefix="repo-", dir=self.d)
        shutil.rmtree(copy)
        shutil.copytree(self.repo, copy)
        return copy

    def run_sign(self, repo, k, *, pub=None, key="default", fpr="default", subid="default", pw="default", extra=None, shell_args=()):
        tmp = tempfile.mkdtemp(dir=self.d)
        env = {"PATH": os.environ["PATH"], "HOME": os.environ.get("HOME", "/tmp"), "TMPDIR": tmp, "TSW_FLATPAK_SIGNING_PIN_FILE": os.devnull}
        for name, val, dflt in (("TSW_FLATPAK_SIGNING_KEY", key, k["ci"]), ("TSW_FLATPAK_SIGNING_FINGERPRINT", fpr, k["fpr"]),
                                ("TSW_FLATPAK_SIGNING_SUBKEY_ID", subid, k["subid"]), ("TSW_FLATPAK_SIGNING_PASSPHRASE", pw, k["pw"])):
            v = dflt if val == "default" else val
            if v is not None:
                env[name] = v
        env.update(extra or {})
        r = subprocess.run(["bash", *shell_args, SIGN, repo, pub or k["pub"]], capture_output=True, text=True, env=env, timeout=900)
        return r, tmp

    def signed(self, repo, k):
        return keys("verify-repo", k["pub"], repo).returncode == 0

    def test_an_unprotected_subkey_from_the_environment_signs_and_the_result_verifies_with_the_committed_public_key(self):
        a = self.a
        repo = self.fresh_repo()
        r, tmp = self.run_sign(repo, a)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("signed and verified", r.stdout)
        self.assertIn("primary secret key not present", r.stdout)
        self.assertTrue(self.signed(repo, a))
        self.assertEqual(keys("verify-repo", self.b["pub"], repo).returncode, 1, "another key must not verify it")

    def test_a_passphrase_protected_subkey_signs_with_the_passphrase_from_the_environment(self):
        for k in (self.b, self.c):  # ed25519 and RSA 4096 with a passphrase (the production subkey is RSA 4096 with a passphrase)
            repo = self.fresh_repo()
            r, tmp = self.run_sign(repo, k)
            self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
            self.assertTrue(self.signed(repo, k))
            self.assertEqual(os.listdir(tmp), [], "the temporary keyring must be removed")

    def test_a_wrong_or_missing_passphrase_is_refused_before_anything_is_signed(self):
        for pw in ("wrong " + PW, None, ""):
            repo = self.fresh_repo()
            r, tmp = self.run_sign(repo, self.b, pw=pw)
            self.assertEqual(r.returncode, 1, (pw, r.stderr))
            self.assertIn("passphrase", r.stderr)
            self.assertFalse(self.signed(repo, self.b), "nothing was signed")
            self.assertEqual(os.listdir(tmp), [])

    def test_neither_the_key_nor_the_passphrase_appears_anywhere(self):
        b = self.b
        repo = self.fresh_repo()
        r, tmp = self.run_sign(repo, b)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        armored = base64.b64decode(b["ci"]).decode()
        body = [l for l in armored.splitlines() if l and not l.startswith(("-----", "Version", "Comment"))]
        for text in (r.stdout, r.stderr):
            self.assertNotIn(b["ci"][:40], text)
            for line in body[:4]:
                self.assertNotIn(line, text)
            self.assertNotIn("PRIVATE KEY", text)
            self.assertNotIn(PW, text)
        hits = subprocess.run(["grep", "-rIl", "-e", "PRIVATE KEY", "-e", PW, repo], capture_output=True, text=True).stdout
        self.assertEqual(hits.strip(), "")
        self.assertEqual(os.listdir(tmp), [])
        # a failing run is as quiet
        r, _ = self.run_sign(self.fresh_repo(), b, pw="wrong " + PW)
        self.assertNotIn(PW, r.stdout + r.stderr)
        self.assertNotIn("wrong " + PW, r.stdout + r.stderr)

    def test_a_missing_secret_or_malformed_variable_is_refused_before_anything_happens(self):
        a = self.a
        for kw in ({"key": None}, {"key": ""}, {"fpr": None}, {"fpr": "not-a-fingerprint"}, {"fpr": a["fpr"][:39]}, {"subid": None}, {"subid": "abc"}, {"subid": a["fpr"][-17:]}):
            repo = self.fresh_repo()
            r, tmp = self.run_sign(repo, a, **kw)
            self.assertEqual(r.returncode, 2, (kw, r.stderr))
            self.assertFalse(self.signed(repo, a), "nothing was signed")
            self.assertEqual(os.listdir(tmp), [])

    def test_garbage_is_not_a_key_and_is_never_echoed(self):
        junk = "AAAA" * 20 + "SECRETLOOKINGTEXT"
        r, tmp = self.run_sign(self.fresh_repo(), self.a, key=junk)
        self.assertEqual(r.returncode, 1)
        self.assertNotIn("SECRETLOOKINGTEXT", r.stdout + r.stderr)
        self.assertEqual(os.listdir(tmp), [])

    def test_the_full_primary_fingerprint_and_the_concrete_subkey_id_must_match_exactly(self):
        a, o = self.a, self.o
        repo = self.fresh_repo()
        for kw, word in (({"fpr": o["fpr"]}, "not the key named"), ({"subid": o["subid"]}, "not the key named"),
                         ({"fpr": a["fpr"][:-1] + ("0" if a["fpr"][-1] != "0" else "1")}, "not the key named"), ({"subid": a["subid"][:-1] + ("0" if a["subid"][-1] != "0" else "1")}, "not the key named")):
            r, _ = self.run_sign(repo, a, **kw)
            self.assertEqual(r.returncode, 1, kw)
            self.assertIn(word, r.stderr)
        r, _ = self.run_sign(repo, a, pub=o["pub"])  # the public key of another project key
        self.assertEqual(r.returncode, 1)
        self.assertIn("committed public key", r.stderr)
        self.assertFalse(self.signed(repo, a), "nothing was signed")

    def test_the_values_pinned_in_the_repository_must_equal_the_variables(self):
        a = self.a
        pin = os.path.join(self.d, "pin.conf")
        with open(pin, "w") as f:
            f.write(f"FLATPAK_SIGNING_FINGERPRINT={a['fpr']}\nFLATPAK_SIGNING_SUBKEY_ID={a['subid']}\n")
        r, _ = self.run_sign(self.fresh_repo(), a, extra={"TSW_FLATPAK_SIGNING_PIN_FILE": pin})
        self.assertEqual(r.returncode, 0, r.stderr)
        for line, word in ((f"FLATPAK_SIGNING_FINGERPRINT={self.o['fpr']}\nFLATPAK_SIGNING_SUBKEY_ID={a['subid']}\n", "fingerprint"),
                           (f"FLATPAK_SIGNING_FINGERPRINT={a['fpr']}\nFLATPAK_SIGNING_SUBKEY_ID={self.o['subid']}\n", "subkey id")):
            with open(pin, "w") as f:
                f.write(line)
            repo = self.fresh_repo()
            r, tmp = self.run_sign(repo, a, extra={"TSW_FLATPAK_SIGNING_PIN_FILE": pin})
            self.assertEqual(r.returncode, 1)
            self.assertIn("pinned", r.stderr)
            self.assertIn(word, r.stderr)
            self.assertFalse(self.signed(repo, a))
            self.assertEqual(os.listdir(tmp), [])

    def test_a_secret_that_carries_the_primary_key_or_more_than_the_signing_subkey_is_refused(self):
        a = self.a
        env = dict(os.environ, GNUPGHOME=a["home"])
        full = subprocess.run(["gpg", "--batch", "--armor", "--export-secret-keys", a["fpr"]], capture_output=True, text=True, env=env).stdout
        self.assertIn("PRIVATE KEY", full)
        repo = self.fresh_repo()
        r, tmp = self.run_sign(repo, a, key=base64.b64encode(full.encode()).decode())
        self.assertEqual(r.returncode, 1)
        self.assertIn("PRIMARY secret key", r.stderr)
        self.assertFalse(self.signed(repo, a))
        self.assertEqual(os.listdir(tmp), [])
        # a second secret subkey (here an encryption subkey) must not travel with it either
        extra = os.path.join(self.d, "gnupg-extra")
        r = keys("generate-split", extra, "TSW extra throw-away <x@example.invalid>")
        fpr = r.stdout.strip().splitlines()[-1]
        subprocess.run(["gpg", "--batch", "--pinentry-mode", "loopback", "--passphrase", "", "--quick-add-key", fpr, "cv25519", "encr", "never"], capture_output=True, env=dict(os.environ, GNUPGHOME=extra), check=True)
        two = subprocess.run(["gpg", "--batch", "--armor", "--export-secret-subkeys", fpr], capture_output=True, text=True, env=dict(os.environ, GNUPGHOME=extra)).stdout
        pub = os.path.join(self.d, "pub-extra.gpg")
        self.assertEqual(keys("export-public", extra, fpr, pub).returncode, 0)
        fps = keys("fingerprints", pub).stdout.split()
        r, _ = self.run_sign(self.fresh_repo(), a, key=base64.b64encode(two.encode()).decode(), fpr=fpr, subid=fps[fps.index("subkey") + 1][-16:], pub=pub)
        self.assertEqual(r.returncode, 1)
        self.assertIn("exactly one signing subkey", r.stderr)
        self.assertEqual(keys("inspect-ci-export", "/nonexistent").returncode, 1)

    def test_a_public_key_file_that_holds_a_secret_is_refused(self):
        a = self.a
        sec = os.path.join(self.d, "secret-as-public.gpg")
        with open(sec, "wb") as f:
            f.write(base64.b64decode(a["ci"]))
        r, _ = self.run_sign(self.fresh_repo(), a, pub=sec)
        self.assertEqual(r.returncode, 1)
        self.assertIn("SECRET", r.stderr)

    def test_tracing_is_refused_because_it_could_print_the_key(self):
        r, _ = self.run_sign(self.fresh_repo(), self.b, shell_args=("-x",))
        self.assertEqual(r.returncode, 2)
        self.assertNotIn(self.b["ci"][:30], r.stdout + r.stderr)
        self.assertNotIn(PW, r.stdout + r.stderr)

    def test_inspect_ci_export_tells_a_correct_export_from_a_dangerous_one(self):
        r = keys("inspect-ci-export", os.path.join(self.d, "ci-b.b64"))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn(self.b["subid"], r.stdout)
        self.assertIn(self.b["fpr"], r.stdout)
        self.assertIn("stub only", r.stdout)
        full = subprocess.run(["gpg", "--batch", "--armor", "--export-secret-keys", self.a["fpr"]], capture_output=True, text=True, env=dict(os.environ, GNUPGHOME=self.a["home"])).stdout
        bad = os.path.join(self.d, "primary.b64")
        with open(bad, "w") as f:
            f.write(base64.b64encode(full.encode()).decode())
        r = keys("inspect-ci-export", bad)
        self.assertEqual(r.returncode, 1)
        self.assertIn("PRIMARY secret key", r.stderr)
        self.assertNotIn("PRIVATE KEY", r.stdout + r.stderr)

    def test_the_script_never_passes_a_secret_as_an_argument_or_writes_it_to_a_file(self):
        with open(SIGN, encoding="utf-8") as f:
            text = f.read()
        code = "\n".join(l for l in text.splitlines() if not l.lstrip().startswith("#"))
        for name in ("TSW_FLATPAK_SIGNING_KEY", "TSW_FLATPAK_SIGNING_PASSPHRASE"):
            for l in [l for l in code.splitlines() if name in l]:
                ok = (re.search(r"printf '%s' \"\$" + name + r"\" \| ", l) or re.search(r"-n \"\$\{" + name + r":-\}\"", l) or re.search(r"unset [A-Z_ ]*" + name, l)
                      or re.search(r"\$\{" + name + r":-\}", l) or "die " in l)
                self.assertTrue(ok, l)
        self.assertNotRegex(code, r'echo[^\n]*TSW_FLATPAK_SIGNING_(KEY|PASSPHRASE)')
        self.assertNotRegex(code, r'(--passphrase\b|-P\s)')
        self.assertNotRegex(code, r'(?m)^\s*set\s+(-[a-z]*x|-o\s+xtrace)')
        self.assertIn("trap cleanup EXIT", code)


if __name__ == "__main__":
    unittest.main()
