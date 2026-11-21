#!/usr/bin/env python3
"""Tests of the scripts that release.yml and the read-only release rehearsal share: release_guard.sh, release_candidate_find.sh, release_notes.py (with stand-ins for `gh` and for the
project's version tools; nothing here touches GitHub). The remaining two (release_candidate_verify.sh, release_assemble_dist.sh) work on real artifacts; their first real run is the
rehearsal on the development repository."""
import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
sys.path.insert(0, HERE)
import release_notes  # noqa: E402

GH_STUB = r"""#!/usr/bin/env python3
import json, os, sys
a = sys.argv[1:]
log = open(os.environ["GH_LOG"], "a"); log.write(" ".join(a) + "\n"); log.close()
if a[:2] == ["release", "list"]:
    for tag in json.loads(os.environ.get("GH_RELEASES", "[]")):
        print(tag)
elif a[:2] == ["run", "list"]:
    if os.environ.get("GH_FAIL_RUN_LIST"):
        sys.exit("HTTP 502")
    print(os.environ.get("GH_RUNS", "[]"))
elif a[0] == "api":
    if os.environ.get("GH_FAIL_API"):
        sys.exit("HTTP 502")
    for item in json.loads(os.environ.get("GH_ARTIFACTS", "[]")):
        name, expired = (item, False) if isinstance(item, str) else item
        print(name + "\t" + ("true" if expired else "false"))
else:
    sys.exit("unexpected gh call: " + " ".join(a))
"""


def run(cmd, cwd, env=None, check=False):
    e = dict(os.environ)
    e.update(env or {})
    return subprocess.run(cmd, cwd=cwd, env=e, capture_output=True, text=True, timeout=60)


class Sandbox(unittest.TestCase):
    """A throw-away git repository with the real shared scripts and stand-ins for the project tools and for `gh`."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.repo = os.path.join(self.tmp, "repo")
        os.makedirs(os.path.join(self.repo, "scripts"))
        for name in ("release_guard.sh", "release_candidate_find.sh"):
            shutil.copy(os.path.join(HERE, name), os.path.join(self.repo, "scripts", name))
        self.write("scripts/version.py", """import sys
a = sys.argv[1:]
if a[:1] == ["check"]: sys.exit(0)
if a[:1] == ["show"]: print("0.4.1")
if a[:1] == ["compare"]:
    x = [int(p) for p in a[1].split(".")]; y = [int(p) for p in a[2].split(".")]
    print(1 if x > y else (0 if x == y else -1))
""")
        self.write("scripts/pin_actions.sh", "#!/usr/bin/env bash\nexit 0\n", 0o755)
        self.bin = os.path.join(self.tmp, "bin")
        os.makedirs(self.bin)
        self.write("../bin/gh", GH_STUB, 0o755)
        self.log = os.path.join(self.tmp, "gh.log")
        for cmd in (["git", "init", "-q", "-b", "main"], ["git", "config", "user.email", "t@example.invalid"], ["git", "config", "user.name", "t"], ["git", "add", "."], ["git", "commit", "-q", "-m", "x"]):
            run(cmd, self.repo)
        self.sha = run(["git", "rev-parse", "HEAD"], self.repo).stdout.strip()
        # origin/main = this commit (a local bare clone stands in for the remote)
        self.remote = os.path.join(self.tmp, "remote.git")
        run(["git", "clone", "-q", "--bare", self.repo, self.remote], self.tmp)
        run(["git", "remote", "add", "origin", self.remote], self.repo)
        run(["git", "fetch", "-q", "origin"], self.repo)

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def write(self, rel, text, mode=0o644):
        p = os.path.normpath(os.path.join(self.repo, rel))
        os.makedirs(os.path.dirname(p), exist_ok=True)
        with open(p, "w") as f:
            f.write(text)
        os.chmod(p, mode)

    def env(self, **extra):
        e = {"PATH": self.bin + os.pathsep + os.environ["PATH"], "GH_LOG": self.log, "GITHUB_SHA": self.sha, "GITHUB_REPOSITORY": "o/r", "GH_TOKEN": "x"}
        e.update(extra)
        return e

    def gh_calls(self):
        return open(self.log).read().splitlines() if os.path.exists(self.log) else []


class Guard(Sandbox):
    def guard(self, *args, **env):
        return run(["bash", "scripts/release_guard.sh", *args], self.repo, self.env(**env))

    def test_a_real_run_accepts_the_matching_tag_on_main_and_reports_the_version(self):
        out = os.path.join(self.tmp, "out")
        r = self.guard(TAG_NAME="v0.4.1", GH_RELEASES=json.dumps(["v0.3.0"]), GITHUB_OUTPUT=out)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("version=0.4.1", open(out).read())
        self.assertIn("0.4.1 is newer than the published release 0.3.0", r.stdout)

    def test_a_real_run_refuses_what_a_release_must_not_be(self):
        for env, why in ((dict(TAG_NAME="0.4.1"), "not a version tag"), (dict(TAG_NAME="v0.4.2"), "does not match APP_VERSION"), (dict(TAG_NAME="v1.0"), "not a version tag"),
                         (dict(TAG_NAME="v0.4.1", GH_RELEASES=json.dumps(["v0.4.1"])), "not newer"), (dict(TAG_NAME="v0.4.1", GH_RELEASES=json.dumps(["v0.5.0", "v0.3.0"])), "not newer")):
            with self.subTest(env=env):
                r = self.guard(**env)
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertIn(why, r.stderr)

    def test_a_commit_that_is_not_on_main_is_refused(self):
        run(["git", "checkout", "-q", "-b", "side"], self.repo)
        self.write("x.txt", "x")
        run(["git", "add", "."], self.repo)
        run(["git", "commit", "-q", "-m", "side"], self.repo)
        sha = run(["git", "rev-parse", "HEAD"], self.repo).stdout.strip()
        r = self.guard(TAG_NAME="v0.4.1", GITHUB_SHA=sha)
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not on main", r.stderr)

    def test_a_tag_that_moved_after_the_checkout_is_refused(self):
        r = self.guard(TAG_NAME="v0.4.1", GITHUB_SHA="0" * 40)
        self.assertEqual(r.returncode, 1)

    def test_drafts_and_pre_releases_are_not_asked_for_and_the_first_release_passes(self):
        r = self.guard(TAG_NAME="v0.4.1", GH_RELEASES="[]")
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("is the first", r.stdout)
        call = next(c for c in self.gh_calls() if c.startswith("release list"))
        self.assertIn("--exclude-drafts", call)
        self.assertIn("--exclude-pre-releases", call)

    def test_the_rehearsal_takes_the_tag_from_the_version_runs_the_same_checks_and_needs_no_tag(self):
        r = self.guard("--rehearse", GH_RELEASES=json.dumps(["v0.3.0"]))
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("is newer than the published release 0.3.0", r.stdout)
        r = self.guard("--rehearse", GH_RELEASES=json.dumps(["v0.4.1"]))
        self.assertEqual(r.returncode, 1)
        self.assertIn("not newer", r.stderr)
        run(["git", "checkout", "-q", "-b", "side"], self.repo)
        self.write("x.txt", "x")
        run(["git", "add", "."], self.repo)
        run(["git", "commit", "-q", "-m", "side"], self.repo)
        r = self.guard("--rehearse", GITHUB_SHA=run(["git", "rev-parse", "HEAD"], self.repo).stdout.strip())
        self.assertEqual(r.returncode, 1)
        self.assertIn("is not on main", r.stderr, "the rehearsal also refuses a commit that is not on main")

    def test_the_guard_only_reads(self):
        self.guard(TAG_NAME="v0.4.1")
        for call in self.gh_calls():
            self.assertTrue(call.startswith("release list"), call)


class CandidateFind(Sandbox):
    KINDS = ("flatpak", "source", "windows", "candidate")

    def find(self, runs, artifacts, **env):
        return run(["bash", "scripts/release_candidate_find.sh"], self.repo, self.env(VERSION="0.4.1", REF_NAME="main", GH_RUNS=json.dumps(runs), GH_ARTIFACTS=json.dumps(artifacts), **env))

    def names(self, kinds=KINDS, sha=None):
        return [f"tsw-forever-0.4.1-{k}-{sha or self.sha}" for k in kinds]

    def good_run(self, **kw):
        d = {"databaseId": 77, "headSha": self.sha, "conclusion": "success", "status": "completed", "workflowName": "build", "createdAt": "2026-11-21T10:00:00Z"}
        d.update(kw)
        return d

    def test_the_successful_build_of_exactly_this_commit_is_found(self):
        out = os.path.join(self.tmp, "out")
        r = self.find([self.good_run(), self.good_run(databaseId=5, createdAt="2026-11-20T10:00:00Z")], self.names(), GITHUB_OUTPUT=out)
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("run_id=77", open(out).read(), "the newest matching run")

    def test_another_commit_another_workflow_or_a_failed_run_is_not_a_candidate(self):
        for run_ in (self.good_run(headSha="f" * 40), self.good_run(workflowName="ci"), self.good_run(conclusion="failure")):
            with self.subTest(run=run_):
                r = self.find([run_], self.names())
                self.assertEqual(r.returncode, 1)
                self.assertTrue("no run of the workflow" in r.stderr or "is not successful" in r.stderr, r.stderr)
        self.assertEqual(self.find([], self.names()).returncode, 1)

    def test_an_older_successful_run_never_stands_in_for_a_newer_one_that_failed_or_is_unfinished(self):
        old = self.good_run(databaseId=5, createdAt="2026-11-20T10:00:00Z")
        for newest, why in ((self.good_run(databaseId=9, conclusion="failure", createdAt="2026-11-21T12:00:00Z"), "failure"),
                            (self.good_run(databaseId=9, conclusion="cancelled", createdAt="2026-11-21T12:00:00Z"), "cancelled"),
                            (self.good_run(databaseId=9, conclusion=None, status="in_progress", createdAt="2026-11-21T12:00:00Z"), "in_progress")):
            with self.subTest(newest=why):
                out = os.path.join(self.tmp, "out-" + why)
                r = self.find([old, newest], self.names(), GITHUB_OUTPUT=out)
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertIn("is not successful", r.stderr)
                self.assertIn("An older run of the same commit is not used", r.stderr)
                self.assertIn("run 9", r.stderr)
                self.assertFalse(os.path.exists(out), "no run id is handed on")

    def test_a_missing_artifact_stops_the_release_and_says_which(self):
        for kind in self.KINDS:
            with self.subTest(kind=kind):
                r = self.find([self.good_run()], self.names([k for k in self.KINDS if k != kind]))
                self.assertEqual(r.returncode, 1)
                self.assertIn(f"-{kind}-", r.stderr)
                self.assertIn("does not exist", r.stderr)
                self.assertIn("gh workflow run build.yml --ref main", r.stderr, "it says what to do")

    def test_an_expired_artifact_is_named_as_expired_with_the_retention_of_both_repositories(self):
        for kind in self.KINDS:
            with self.subTest(kind=kind):
                artifacts = [[n, True] if n.split("-")[3] == kind else n for n in self.names()]
                r = self.find([self.good_run()], artifacts)
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertIn(f"-{kind}-", r.stderr)
                self.assertIn("has expired", r.stderr)
                self.assertIn("7 days in the development repository", r.stderr)

    def test_artifacts_of_another_commit_or_version_never_count(self):
        other = "e" * 40
        r = self.find([self.good_run()], self.names(sha=other))
        self.assertEqual(r.returncode, 1)
        self.assertIn("does not exist", r.stderr)
        r = self.find([self.good_run()], [n.replace("0.4.1", "0.4.0") for n in self.names()])
        self.assertEqual(r.returncode, 1)
        # a stray artifact of another run next to the right ones changes nothing
        r = self.find([self.good_run()], self.names() + self.names(sha=other))
        self.assertEqual(r.returncode, 0, r.stderr)

    def test_a_failing_github_answer_stops_with_a_message_and_a_wrong_commit_id_is_refused(self):
        r = self.find([self.good_run()], self.names(), GH_FAIL_RUN_LIST="1")
        self.assertEqual(r.returncode, 1)
        self.assertIn("cannot ask GitHub for the build runs", r.stderr)
        r = self.find([self.good_run()], self.names(), GH_FAIL_API="1")
        self.assertEqual(r.returncode, 1)
        self.assertIn("cannot list the artifacts", r.stderr)
        for bad in ("abc", "G" * 40, self.sha[:39], self.sha.upper()):
            r = self.find([self.good_run()], self.names(), GITHUB_SHA=bad)
            self.assertEqual(r.returncode, 1, bad)
            self.assertIn("not a full commit id", r.stderr)

    def test_it_reads_only_and_asks_for_the_runs_of_this_commit_and_for_the_state_of_the_artifacts(self):
        self.find([self.good_run()], self.names())
        calls = self.gh_calls()
        self.assertTrue(calls and all(c.startswith(("run list", "api ")) for c in calls), calls)
        self.assertTrue(any("--commit " + self.sha in c and "--workflow build.yml" in c for c in calls))
        self.assertFalse(any("--status" in c for c in calls), "all runs are listed: the newest one decides")
        self.assertTrue(any(".expired" in c for c in calls))


class CandidateVerify(unittest.TestCase):
    """release_candidate_verify.sh with REAL fixtures made by release_candidate.py create."""

    SHA = "a" * 40
    V = "0.4.1"

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        os.symlink(HERE, os.path.join(self.tmp, "scripts"))
        self.make_all()

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def make_all(self, sha=None, version=None):
        sha, v = sha or self.SHA, version or self.V
        for kind, files in (("flatpak", [f"tsw-forever-{v}.flatpak", "SHA256SUMS-flatpak"]), ("source", [f"tsw-forever-{v}-src.tar.gz", "SHA256SUMS-source"]),
                            ("windows", [f"tsw-forever-{v}-windows-setup.exe", "SHA256SUMS-windows"])):
            d = os.path.join(self.tmp, "all", f"tsw-forever-{v}-{kind}-{sha}")
            os.makedirs(d)
            for f in files:
                with open(os.path.join(d, f), "wb") as fh:
                    fh.write((kind + f).encode() * 10)
        parts = os.path.join(self.tmp, "mk")
        shutil.rmtree(parts, ignore_errors=True)
        os.makedirs(parts)
        for d in os.listdir(os.path.join(self.tmp, "all")):
            if d.startswith(f"tsw-forever-{v}-") and d.endswith(sha) and "-candidate-" not in d:
                shutil.copytree(os.path.join(self.tmp, "all", d), os.path.join(parts, d))
        cd = os.path.join(self.tmp, "all", f"tsw-forever-{v}-candidate-{sha}")
        os.makedirs(cd)
        r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "release_candidate.py"), "create", parts, "--version", v, "--commit", sha, "--out", os.path.join(cd, "candidate.json")], capture_output=True, text=True)
        assert r.returncode == 0, r.stderr

    def verify(self, **env):
        e = dict(os.environ, VERSION=self.V, GITHUB_SHA=self.SHA)
        e.update(env)
        return subprocess.run(["bash", "scripts/release_candidate_verify.sh"], cwd=self.tmp, capture_output=True, text=True, env=e)

    def artifact(self, kind, sha=None):
        return os.path.join(self.tmp, "all", f"tsw-forever-{self.V}-{kind}-{sha or self.SHA}")

    def test_the_real_candidate_verifies_and_is_sorted_into_manifest_and_parts(self):
        r = self.verify()
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertIn("candidate verified", r.stdout)
        self.assertTrue(os.path.isfile(os.path.join(self.tmp, "manifest", "candidate.json")))
        self.assertEqual(sorted(os.listdir(os.path.join(self.tmp, "parts"))), sorted(f"tsw-forever-{self.V}-{k}-{self.SHA}" for k in ("flatpak", "source", "windows")))

    def test_a_missing_artifact_says_which_one_and_what_to_do(self):
        for kind in ("flatpak", "source", "windows", "candidate"):
            with self.subTest(kind=kind):
                self.tearDown()
                self.setUp()
                shutil.rmtree(self.artifact(kind))
                r = self.verify()
                self.assertEqual(r.returncode, 1)
                self.assertIn(f"tsw-forever-{self.V}-{kind}-{self.SHA}", r.stderr)
                self.assertIn("expired, was deleted, or never existed", r.stderr)
                self.assertIn("never used", r.stderr)
                self.assertFalse(os.path.exists(os.path.join(self.tmp, "parts")), "nothing is sorted before everything is present")

    def test_nothing_downloaded_at_all_is_an_error_with_the_likely_reason(self):
        shutil.rmtree(os.path.join(self.tmp, "all"))
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertIn("were not downloaded", r.stderr)
        self.assertIn("expired", r.stderr)

    def test_artifacts_of_another_commit_or_version_are_not_taken_instead(self):
        other = "b" * 40
        shutil.rmtree(os.path.join(self.tmp, "all"))
        self.make_all(sha=other)  # a complete, valid candidate - but of ANOTHER commit
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertIn("was not downloaded", r.stderr)
        shutil.rmtree(os.path.join(self.tmp, "all"))
        self.make_all(version="0.4.0")
        self.assertEqual(self.verify().returncode, 1)

    def test_an_additional_artifact_a_loose_file_or_a_symbolic_link_are_refused(self):
        os.makedirs(os.path.join(self.tmp, "all", "tsw-forever-0.4.1-flatpak-" + "c" * 40))
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertIn("unexpected artifact", r.stderr)
        shutil.rmtree(os.path.join(self.tmp, "all", "tsw-forever-0.4.1-flatpak-" + "c" * 40))
        open(os.path.join(self.tmp, "all", "loose.txt"), "w").write("x")
        self.assertEqual(self.verify().returncode, 1)
        os.remove(os.path.join(self.tmp, "all", "loose.txt"))
        os.symlink("/etc/hostname", os.path.join(self.artifact("source"), "link"))
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertIn("symbolic link", r.stderr)

    def test_a_changed_byte_a_missing_or_extra_file_and_a_foreign_manifest_stop_everything(self):
        with open(os.path.join(self.artifact("windows"), f"tsw-forever-{self.V}-windows-setup.exe"), "ab") as f:
            f.write(b"x")
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertNotIn("candidate verified", r.stdout)
        self.tearDown()
        self.setUp()
        os.remove(os.path.join(self.artifact("flatpak"), "SHA256SUMS-flatpak"))
        self.assertEqual(self.verify().returncode, 1)
        self.tearDown()
        self.setUp()
        open(os.path.join(self.artifact("source"), "extra.bin"), "w").write("x")
        self.assertEqual(self.verify().returncode, 1)
        self.tearDown()
        self.setUp()
        open(os.path.join(self.artifact("candidate"), "second.json"), "w").write("{}")
        r = self.verify()
        self.assertEqual(r.returncode, 1)
        self.assertIn("exactly candidate.json", r.stderr)

    def test_a_manifest_of_another_commit_or_version_is_refused(self):
        path = os.path.join(self.artifact("candidate"), "candidate.json")
        doc = json.load(open(path))
        for key, value in (("commit", "d" * 40), ("version", "9.9.9")):
            with self.subTest(key=key):
                self.tearDown()
                self.setUp()
                path = os.path.join(self.artifact("candidate"), "candidate.json")
                d = json.load(open(path))
                d[key] = value
                json.dump(d, open(path, "w"))
                r = self.verify()
                self.assertEqual(r.returncode, 1, r.stdout)
                self.assertNotIn("candidate verified", r.stdout)
        self.assertIn("commit", doc)

    def test_the_commit_id_must_be_a_full_lower_case_sha(self):
        for bad in ("abc", "A" * 40, "g" * 40):
            r = self.verify(GITHUB_SHA=bad)
            self.assertEqual(r.returncode, 1, bad)
            self.assertIn("not a full commit id", r.stderr)


class ReleaseNotes(unittest.TestCase):
    CHANGELOG = "# Changelog\n\n## [Unreleased]\n\n## [0.4.1] – 2026-11-21\n\nIntro text.\n\n### Changed\n\n- one\n\n## [0.4.0] – 2026-11-20\n\n- old\n"

    def test_the_entry_of_the_version_is_extracted_without_its_neighbours(self):
        notes = release_notes.extract(self.CHANGELOG, "0.4.1")
        self.assertTrue(notes.startswith("# TSW-Forever 0.4.1\n\nIntro text."))
        self.assertIn("- one", notes)
        self.assertNotIn("old", notes)
        self.assertNotIn("0.4.0", notes)

    def test_a_missing_or_empty_entry_fails(self):
        self.assertIsNone(release_notes.extract(self.CHANGELOG, "0.9.9"))
        self.assertIsNone(release_notes.extract("## [1.0.0] x\n\n## [0.9.0] y\n- z\n", "1.0.0"))

    def test_the_command_line_takes_a_tag_or_a_version(self):
        tmp = tempfile.mkdtemp()
        try:
            open(os.path.join(tmp, "CHANGELOG.md"), "w").write(self.CHANGELOG)
            for arg in ("v0.4.1", "0.4.1"):
                r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "release_notes.py"), arg, "--changelog", os.path.join(tmp, "CHANGELOG.md"), "--out", os.path.join(tmp, "n.md")], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, r.stderr)
            self.assertTrue(open(os.path.join(tmp, "n.md")).read().startswith("# TSW-Forever 0.4.1"))
            r = subprocess.run([sys.executable, "-I", os.path.join(HERE, "release_notes.py"), "v9.9.9", "--changelog", os.path.join(tmp, "CHANGELOG.md"), "--out", os.path.join(tmp, "m.md")], capture_output=True, text=True)
            self.assertEqual(r.returncode, 1)
            self.assertFalse(os.path.exists(os.path.join(tmp, "m.md")))
        finally:
            shutil.rmtree(tmp)

    def test_the_real_changelog_has_an_entry_for_the_current_version(self):
        import re
        conf = open(os.path.join(ROOT, "packaging", "project.conf"), encoding="utf-8").read()
        version = re.search(r"^APP_VERSION=(\S+)", conf, re.M).group(1)
        self.assertIsNotNone(release_notes.extract(open(os.path.join(ROOT, "CHANGELOG.md"), encoding="utf-8").read(), version))


class SharedScriptsAreThere(unittest.TestCase):
    def test_the_scripts_are_executable_and_the_workflows_use_them(self):
        for name in ("release_guard.sh", "release_candidate_find.sh", "release_candidate_verify.sh", "release_assemble_dist.sh", "release_notes.py"):
            self.assertTrue(os.access(os.path.join(HERE, name), os.X_OK), name)
            for wf in ("release.yml", "release-rehearsal.yml"):
                self.assertIn(f"scripts/{name}", open(os.path.join(ROOT, ".github", "workflows", wf), encoding="utf-8").read(), f"{wf} uses {name}")


if __name__ == "__main__":
    unittest.main()
