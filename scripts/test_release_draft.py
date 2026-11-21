#!/usr/bin/env python3
"""scripts/release_draft.sh against a STUB of the gh program (no network, no GitHub): the draft is filled and verified, a published release is never touched, a tampered or
incomplete draft is never published, and the publication is read back."""
import hashlib
import json
import os
import shutil
import subprocess
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
SCRIPT = os.path.join(HERE, "release_draft.sh")
V = "1.2.3"
NAMES = [f"tsw-forever-{V}.flatpak", f"tsw-forever-{V}-src.tar.gz", f"tsw-forever-{V}-windows-setup.exe"]

STUB = r'''#!/usr/bin/env python3
import json, os, shutil, sys
state_file = os.environ["STUB_STATE"]
st = json.load(open(state_file)) if os.path.exists(state_file) else {"releases": {}}
a = sys.argv[1:]
def save():
    json.dump(st, open(state_file, "w"))
def fail(msg):
    sys.stderr.write(msg + "\n"); sys.exit(1)
st.setdefault("log", []).append(a[:3])
if a[:2] == ["release", "view"]:
    r = st["releases"].get(a[2])
    if not r: fail("release not found")
    print(json.dumps({"isDraft": r["draft"], "isPrerelease": r["pre"], "assets": [{"name": n} for n in r["assets"]]}))
elif a[:2] == ["release", "create"]:
    if a[2] in st["releases"]: fail("exists")
    if "--verify-tag" not in a or "--draft" not in a: fail("must be a draft for an existing tag")
    st["releases"][a[2]] = {"draft": True, "pre": False, "assets": {}}
elif a[:2] == ["release", "upload"]:
    r = st["releases"][a[2]]
    for f in [x for x in a[3:] if not x.startswith("--")]:
        n = os.path.basename(f)
        if n in r["assets"] and "--clobber" not in a: fail("asset exists")
        data = open(f, "rb").read()
        if os.environ.get("STUB_CORRUPT") == n: data = data + b"x"
        r["assets"][n] = data.hex()
elif a[:2] == ["release", "download"]:
    r = st["releases"][a[2]]
    d = a[a.index("--dir") + 1]
    for n, h in r["assets"].items():
        open(os.path.join(d, n), "wb").write(bytes.fromhex(h))
elif a[:2] == ["release", "edit"]:
    r = st["releases"][a[2]]
    if "--draft=false" in a: r["draft"] = False
    if os.environ.get("STUB_PRERELEASE"): r["pre"] = True
else:
    fail("unsupported " + " ".join(a))
save()
'''


class ReleaseDraft(unittest.TestCase):
    def setUp(self):
        self.d = tempfile.mkdtemp(prefix="tsw-draft-")
        self.addCleanup(shutil.rmtree, self.d, True)
        self.dist = os.path.join(self.d, "dist")
        os.makedirs(self.dist)
        for n in NAMES:
            with open(os.path.join(self.dist, n), "wb") as f:
                f.write(n.encode() * 10)
        with open(os.path.join(self.dist, "SHA256SUMS"), "w") as f:
            for n in NAMES:
                f.write(hashlib.sha256(open(os.path.join(self.dist, n), "rb").read()).hexdigest() + "  " + n + "\n")
        self.notes = os.path.join(self.d, "notes.md")
        with open(self.notes, "w") as f:
            f.write("notes\n")
        self.gh = os.path.join(self.d, "gh")
        with open(self.gh, "w") as f:
            f.write(STUB)
        os.chmod(self.gh, 0o755)
        self.state = os.path.join(self.d, "state.json")

    def run_s(self, *args, env=None):
        e = dict(os.environ, GH=self.gh, STUB_STATE=self.state)
        e.update(env or {})
        return subprocess.run(["bash", SCRIPT, *args], capture_output=True, text=True, env=e)

    def st(self):
        return json.load(open(self.state))

    def test_upload_creates_a_draft_with_exactly_the_four_files_and_nothing_is_published(self):
        r = self.run_s("upload", f"v{V}", self.notes, self.dist)
        self.assertEqual(r.returncode, 0, r.stderr)
        rel = self.st()["releases"][f"v{V}"]
        self.assertTrue(rel["draft"])
        self.assertEqual(sorted(rel["assets"]), sorted(NAMES + ["SHA256SUMS"]))

    def test_upload_continues_an_existing_draft_and_refuses_a_published_release(self):
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0)
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0, "a re-run continues the draft")
        self.assertEqual(self.run_s("publish", f"v{V}", self.dist).returncode, 0)
        r = self.run_s("upload", f"v{V}", self.notes, self.dist)
        self.assertEqual(r.returncode, 1)
        self.assertIn("PUBLISHED", r.stderr)

    def test_a_file_that_is_changed_on_the_way_is_caught_by_the_read_back(self):
        r = self.run_s("upload", f"v{V}", self.notes, self.dist, env={"STUB_CORRUPT": NAMES[0]})
        self.assertEqual(r.returncode, 1)
        self.assertIn("differs", r.stderr)

    def test_a_draft_with_a_foreign_file_is_a_hard_stop(self):
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0)
        s = self.st()
        s["releases"][f"v{V}"]["assets"]["stray.bin"] = "00"
        json.dump(s, open(self.state, "w"))
        r = self.run_s("upload", f"v{V}", self.notes, self.dist)
        self.assertEqual(r.returncode, 1)
        self.assertIn("do not belong", r.stderr)

    def test_publish_needs_a_verified_draft_and_reads_the_result_back(self):
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0)
        r = self.run_s("publish", f"v{V}", self.dist)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertFalse(self.st()["releases"][f"v{V}"]["draft"])

    def test_a_draft_changed_after_the_verification_is_not_published(self):
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0)
        s = self.st()
        s["releases"][f"v{V}"]["assets"][NAMES[0]] = "00"
        json.dump(s, open(self.state, "w"))
        r = self.run_s("publish", f"v{V}", self.dist)
        self.assertEqual(r.returncode, 1)
        self.assertTrue(self.st()["releases"][f"v{V}"]["draft"], "still a draft")
        self.assertNotIn(["release", "edit", f"v{V}"], self.st()["log"])

    def test_publish_refuses_a_missing_release_and_an_already_public_one_and_a_prerelease_result(self):
        self.assertEqual(self.run_s("publish", f"v{V}", self.dist).returncode, 1)
        self.assertEqual(self.run_s("upload", f"v{V}", self.notes, self.dist).returncode, 0)
        r = self.run_s("publish", f"v{V}", self.dist, env={"STUB_PRERELEASE": "1"})
        self.assertEqual(r.returncode, 1)
        self.assertIn("public prerelease", r.stderr)

    def test_a_wrong_dist_folder_or_tag_is_refused_before_gh_is_called(self):
        for tag in ("1.2.3", "v1.2", "v1.2.3-rc1", "v01.2.3"):
            self.assertEqual(self.run_s("upload", tag, self.notes, self.dist).returncode, 2, tag)
        extra = os.path.join(self.dist, "extra.txt")
        with open(extra, "w") as f:
            f.write("x")
        r = self.run_s("upload", f"v{V}", self.notes, self.dist)
        self.assertEqual(r.returncode, 1)
        self.assertFalse(os.path.exists(self.state), "gh was never called")
        os.remove(extra)
        with open(os.path.join(self.dist, NAMES[1]), "ab") as f:
            f.write(b"tampered")
        r = self.run_s("upload", f"v{V}", self.notes, self.dist)
        self.assertEqual(r.returncode, 1)
        self.assertIn("SHA256SUMS", r.stderr)
        self.assertFalse(os.path.exists(self.state))


if __name__ == "__main__":
    unittest.main()
