#!/usr/bin/env python3
"""scripts/flatpak_release_offline.sh: the update check inside the release flow. The REAL release script, flatpak_keys.sh and check_flatpak_delta_update.sh run in a temporary tree; only the heavy
part (the Flatpak build of the launcher) is a stub that makes a tiny application repository with a history, optionally with a deliberately broken delta path. Proves that

* a release whose deltas are signed goes through and is bundled (also the very first release, without history);
* a delta generated before the signing (an invalid delta path) stops the release with exit 21 BEFORE anything is bundled;
* a key rotation is refused until the previous public key is named explicitly (FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS), is then accepted with a visible WARNING, and a missing key file stops the release
  before the build starts;
* the call sits after the signature check and before the bundle in the real script, and never switches the GPG verification off.
Throw-away keys only; everything lives in temporary folders that this test removes again. Skipped where flatpak, gpg or ostree are not installed."""
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
RELEASE = os.path.join(HERE, "flatpak_release_offline.sh")

VERSION_STUB = """import sys
if sys.argv[1:] == ["check"]:
    print("ok")
elif sys.argv[1:] == ["show"]:
    print("0.0.0")
else:
    sys.exit(2)
"""

# the stand-in for scripts/build_flatpak.sh: `stable-repo` writes a small UNSIGNED application repository with the history the scenario asks for, `bundle` only leaves a marker
BUILD_STUB = r"""#!/bin/bash
set -euo pipefail
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"
export_release() { head -c 300000 /dev/urandom > "$TSW_STUB_APP/files/payload.bin"; flatpak build-export build/flatpak/repo "$TSW_STUB_APP" stable >/dev/null; }
case "${1:-}" in
  stable-repo)
    touch build-stub-stable-repo-ran
    mkdir -p build/flatpak; rm -rf build/flatpak/repo
    if [ -n "${TSW_STUB_HISTORY_HOME:-}" ]; then
      export_release                                   # release 1: the mirrored, already published history, signed with the key it was published with
      scripts/flatpak_keys.sh sign-repo "$TSW_STUB_HISTORY_HOME" "$TSW_STUB_HISTORY_FPR" build/flatpak/repo >/dev/null
    fi
    export_release                                     # the new release, unsigned
    if [ "${TSW_STUB_PRESIGN_DELTAS:-0}" = 1 ]; then   # the mistake: static deltas before the signing
      flatpak build-update-repo --generate-static-deltas build/flatpak/repo >/dev/null
    fi
    echo "unsigned stable repository (stub)";;
  bundle)
    touch build-stub-bundle-ran
    echo stub-bundle > build/flatpak/tsw-launcher.flatpak;;
  *) exit 2;;
esac
"""


def sh(*args, **kw):
    return subprocess.run(list(args), capture_output=True, text=True, **kw)


@requires_tools("flatpak", "gpg", "ostree", "python3", "git", "tar", "sha256sum")
class ReleaseDeltaCheck(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = tempfile.mkdtemp(prefix="tsw-release-")
        keys = os.path.join(HERE, "flatpak_keys.sh")
        cls.keys = {}
        for n in ("a", "b"):
            home = os.path.join(cls.d, "keys", "gnupg-" + n)
            r = sh("bash", keys, "generate-split", home, f"TSW release test {n} <{n}@example.invalid>", "1d")
            assert r.returncode == 0, r.stderr
            fpr = r.stdout.strip().splitlines()[-1]
            pub = os.path.join(cls.d, "keys", f"pub-{n}.gpg")
            assert sh("bash", keys, "export-public", home, fpr, pub).returncode == 0
            cls.keys[n] = (home, fpr, pub)
        cls.app = os.path.join(cls.d, "app")
        os.makedirs(os.path.join(cls.app, "files"))
        os.makedirs(os.path.join(cls.app, "export"))
        with open(os.path.join(cls.app, "metadata"), "w") as f:
            f.write("[Application]\nname=org.tswtest.Release\nruntime=org.freedesktop.Platform/x86_64/25.08\nsdk=org.freedesktop.Sdk/x86_64/25.08\ncommand=true\n")
        cls.count = 0

    @classmethod
    def tearDownClass(cls):
        for home, _f, _p in cls.keys.values():
            subprocess.run(["gpgconf", "--homedir", home, "--kill", "all"], capture_output=True)
        shutil.rmtree(cls.d, ignore_errors=True)

    def tree(self):
        """A fresh temporary project tree: the real scripts, a version stub and the build stub."""
        type(self).count += 1
        root = os.path.join(self.d, f"root{self.count}")
        os.makedirs(os.path.join(root, "scripts"))
        for name in ("flatpak_release_offline.sh", "flatpak_keys.sh", "check_flatpak_delta_update.sh"):
            shutil.copy(os.path.join(HERE, name), os.path.join(root, "scripts", name))
        with open(os.path.join(root, "scripts", "version.py"), "w") as f:
            f.write(VERSION_STUB)
        stub = os.path.join(root, "scripts", "build_flatpak.sh")
        with open(stub, "w") as f:
            f.write(BUILD_STUB)
        os.chmod(stub, 0o755)
        return root

    def release(self, signer="a", history="a", presign=False, extra=None):
        """Run the real release script with the key `signer`; `history` signed the mirrored first release ("" = a first release without history)."""
        root = self.tree()
        home, fpr, pub = self.keys[signer]
        env = dict(os.environ, FLATPAK_STABLE_KEY=pub, TSW_STUB_APP=self.app, TSW_STUB_PRESIGN_DELTAS="1" if presign else "0")
        env.pop("FLATPAK_LIVE_REPO_URL", None)
        env.pop("FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS", None)
        if history:
            env.update(TSW_STUB_HISTORY_HOME=self.keys[history][0], TSW_STUB_HISTORY_FPR=self.keys[history][1])
        if extra is not None:
            env["FLATPAK_RELEASE_EXTRA_TRUSTED_KEYS"] = extra
        res = subprocess.run(["bash", os.path.join(root, "scripts", "flatpak_release_offline.sh"), home, fpr, "--allow-dirty", "--allow-untagged"], capture_output=True, text=True, env=env, cwd=root)
        return root, res

    def assertBundled(self, root, res):
        self.assertEqual(res.returncode, 0, res.stdout + res.stderr)
        self.assertTrue(os.path.exists(os.path.join(root, "build-stub-bundle-ran")), "the bundle step ran")
        dest = os.path.join(root, "build", "flatpak-release", "0.0.0")
        for name in ("tsw-forever-0.0.0.flatpak", "tsw-forever-0.0.0-flatpak-repo.tar.gz", "SHA256SUMS-flatpak"):
            self.assertTrue(os.path.isfile(os.path.join(dest, name)), name)
        self.assertLess(res.stdout.index("all checks passed"), res.stdout.index("SHA256SUMS") if "SHA256SUMS" in res.stdout else len(res.stdout), "the check ran before the packaging")

    def assertStopped(self, root, res, message):
        self.assertEqual(res.returncode, 21, res.stdout + res.stderr)
        self.assertIn(message, res.stderr)
        self.assertFalse(os.path.exists(os.path.join(root, "build-stub-bundle-ran")), "nothing is bundled")
        self.assertFalse(os.path.exists(os.path.join(root, "build", "flatpak-release", "0.0.0")), "no release folder is written")

    def test_a_release_with_signed_deltas_is_checked_and_bundled(self):
        root, res = self.release()
        self.assertBundled(root, res)
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 2, "the delta from scratch and from the previous release: " + res.stdout)
        for text in ("also trusting", "key rotation"):
            self.assertNotIn(text, res.stderr + res.stdout, "no extra key was configured, so nothing is trusted beyond this release's key")

    def test_a_first_release_without_history_is_checked_and_bundled(self):
        root, res = self.release(history="")
        self.assertBundled(root, res)
        self.assertEqual(res.stdout.count("updated over the signed delta path"), 1, res.stdout)

    def test_an_invalid_delta_path_stops_the_release_before_anything_is_bundled(self):
        root, res = self.release(presign=True)
        self.assertStopped(root, res, "cannot be updated over its static deltas")
        self.assertIn("no signatures found", res.stdout, "the reason is shown")
        self.assertIn("verified: 1 ref", res.stdout, "verify-repo had passed: only the new check sees the delta")

    def test_a_key_rotation_is_refused_until_the_previous_key_is_named(self):
        root, res = self.release(signer="b", history="a")
        self.assertStopped(root, res, "cannot be updated over its static deltas")
        self.assertIn("the older commit could not be fetched and verified first", res.stdout)
        for text in ("also trusting", "key rotation"):
            self.assertNotIn(text, res.stdout + res.stderr, "nothing is trusted silently")

    def test_a_key_rotation_with_the_previous_key_named_is_accepted_with_a_warning(self):
        root, res = self.release(signer="b", history="a", extra=self.keys["a"][2])
        self.assertBundled(root, res)
        self.assertIn("WARNING: key rotation", res.stderr)
        self.assertIn("WARNING: also trusting, for OLDER commits only", res.stdout)
        self.assertIn("verifies with the given key alone", res.stdout, "the new state was verified with this release's key alone")

    def test_a_missing_extra_key_file_stops_the_release_before_the_build(self):
        root, res = self.release(signer="b", history="a", extra=os.path.join(self.d, "missing.gpg"))
        self.assertEqual(res.returncode, 2, res.stdout + res.stderr)
        self.assertIn("is empty or missing", res.stderr)
        self.assertFalse(os.path.exists(os.path.join(root, "build-stub-stable-repo-ran")), "the build never started")

    def test_the_call_sits_between_the_signature_check_and_the_bundle_and_never_disables_gpg(self):
        src = open(RELEASE, encoding="utf-8").read()
        code = "\n".join(l for l in src.splitlines() if not l.lstrip().startswith("#"))
        verify = code.index('scripts/flatpak_keys.sh verify-repo "$pub" build/flatpak/repo')
        call = code.index("scripts/check_flatpak_delta_update.sh --require-delta")
        bundle = code.index("scripts/build_flatpak.sh bundle")
        self.assertLess(verify, call)
        self.assertLess(call, bundle)
        line = next(l for l in code.splitlines() if "check_flatpak_delta_update.sh" in l)
        self.assertIn('build/flatpak/repo "$pub" || die', line, "the key is the release's public key and a failure dies")
        self.assertTrue(line.rstrip().endswith("21"), "exit 21")
        for forbidden in ("gpg-verify=false", "--no-gpg-verify", "gpg-verify-summary=false"):
            self.assertNotIn(forbidden, src)


if __name__ == "__main__":
    unittest.main()
