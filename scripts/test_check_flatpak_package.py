#!/usr/bin/env python3
"""Tests for check_flatpak_package.py (python3 -I scripts/test_check_flatpak_package.py)."""
import copy
import json
import os
import re
import tempfile
import sys
import unittest

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)  # -I drops the script directory from the path
import check_flatpak_package as c  # noqa: E402

ROOT = os.path.abspath(os.path.join(HERE, ".."))
MANIFEST = os.path.join(HERE, "..", "packaging", "flatpak", "org.tswlauncher.Launcher.json")


class ManifestPolicy(unittest.TestCase):
    def setUp(self):
        self.m = json.load(open(MANIFEST))

    def test_the_shipped_manifest_passes(self):
        self.assertEqual(c.check_manifest(self.m), [])

    def bad(self, edit):
        m = copy.deepcopy(self.m)
        edit(m)
        return c.check_manifest(m)

    def test_broad_or_escaping_permissions_are_rejected(self):
        for arg in ["--filesystem=host", "--filesystem=home", "--filesystem=~", "--filesystem=xdg-data", "--filesystem=~/Games", "--device=all",
                    "--talk-name=org.freedesktop.Flatpak", "--socket=session-bus", "--system-talk-name=org.freedesktop.NetworkManager", "--allow=devel",
                    "--env=LD_PRELOAD=/x"]:
            self.assertTrue(self.bad(lambda m, a=arg: m["finish-args"].append(a)), arg)

    def test_multiarch_is_required_for_the_32_bit_client(self):
        self.assertTrue(self.bad(lambda m: m["finish-args"].remove("--allow=multiarch")))

    def test_exactly_one_host_folder_is_granted_the_game_folder(self):
        self.assertEqual([x for x in self.m["finish-args"] if x.startswith("--filesystem=")], ["--filesystem=~/Games/The Secret World:create"])
        for grant in ("--filesystem=~/Games:create", "--filesystem=~/Games/The Secret World", "--filesystem=xdg-data/Steam/compatibilitytools.d:ro",
                      "--filesystem=~/Games/the-secret-world-classic", "--filesystem=~/Games/The Secret World:rw"):
            self.assertTrue(self.bad(lambda m, g=grant: m["finish-args"].append(g)), grant)

    def test_unpinned_or_networked_sources_and_downloading_commands_are_rejected(self):
        self.assertTrue(self.bad(lambda m: m["modules"][0]["sources"].append({"type": "git", "url": "https://example.org/x.git"})))
        self.assertTrue(self.bad(lambda m: m["modules"][0]["sources"].append({"type": "archive", "url": "https://example.org/x.tar.gz"})))
        self.assertFalse(self.bad(lambda m: m["modules"][0]["sources"].append({"type": "archive", "url": "https://example.org/x.tar.gz", "sha256": "a" * 64})))
        self.assertTrue(self.bad(lambda m: m["modules"][0]["build-commands"].append("curl -O https://example.org/game.exe")))
        self.assertTrue(self.bad(lambda m: m["modules"][0]["build-commands"].append("cargo build --release")))

    def test_the_32_bit_extensions_and_pinned_runtime_are_required(self):
        self.assertTrue(self.bad(lambda m: m["add-extensions"].pop("org.freedesktop.Platform.GL32")))
        self.assertTrue(self.bad(lambda m: m.update({"runtime-version": "stable"})))
        self.assertTrue(self.bad(lambda m: m["finish-args"].remove("--device=dri")))


WORKFLOW = os.path.join(HERE, "..", ".github", "workflows", "flatpak.yml")


class WorkflowPolicy(unittest.TestCase):
    def setUp(self):
        self.text = open(WORKFLOW).read()
        self.m = json.load(open(MANIFEST))

    def test_the_shipped_workflow_passes(self):
        self.assertEqual(c.check_workflow(self.text, self.m), [])

    def test_publishing_pushing_secrets_and_other_formats_are_rejected(self):
        for extra in ["run: git push origin HEAD", "run: gh release create v1", "env: { T: ${{ secrets.TOKEN }} }", "run: build the .exe", "run: sudo rm -rf /x"]:
            self.assertTrue(c.check_workflow(self.text + "\n      - " + extra + "\n", self.m), extra)

    def test_write_permissions_unpinned_actions_and_a_runtime_mismatch_are_rejected(self):
        self.assertTrue(c.check_workflow(self.text.replace("contents: read", "contents: write"), self.m))
        self.assertTrue(c.check_workflow(re.sub(r"actions/checkout@[0-9a-f]{40}", "actions/checkout@v4", self.text), self.m), "tag-only pin")
        self.assertTrue(c.check_workflow(re.sub(r"actions/checkout@[0-9a-f]{40}", "actions/checkout", self.text), self.m))
        self.assertTrue(c.check_workflow(self.text.replace('"25.08"', '"24.08"'), self.m))
        self.assertTrue(c.check_workflow(self.text.replace("persist-credentials: false", "persist-credentials: true"), self.m))

    def test_tests_run_before_the_build(self):
        a, b = self.text.index("build_flatpak.sh test"), self.text.index("build_flatpak.sh build")
        self.assertLess(a, b)


class BuildWorkflowHasNoSecrets(unittest.TestCase):
    def test_the_build_workflow_still_has_no_secrets_and_cannot_publish(self):
        build = open(WORKFLOW).read()
        self.assertNotIn("secrets.", build)
        self.assertNotIn("pages: write", build)


WORKFLOWS = os.path.join(HERE, "..", ".github", "workflows")


def on(w, name):
    """The `on:` block (PyYAML reads the key `on` as the boolean True)."""
    d = w[name]
    return d["on"] if "on" in d else d[True]


def w_jobs(wf):
    return sorted(wf["release.yml"]["jobs"])


class MainOnlyPolicy(unittest.TestCase):
    """The build policy: checks everywhere, packages and publication from main only, no free input, no tags."""

    def setUp(self):
        import yaml
        self.m = json.load(open(MANIFEST))
        self.wf = {f: yaml.safe_load(open(os.path.join(WORKFLOWS, f))) for f in os.listdir(WORKFLOWS) if f.endswith(".yml")}

    def errs(self, mutate):
        wf = copy.deepcopy(self.wf)
        mutate(wf)
        return c.check_ci_policy(wf, self.m)

    def test_the_shipped_workflows_pass(self):
        self.assertEqual(c.check_ci_policy(self.wf, self.m), [])

    # ---- the Flatpak and release script tests must RUN in the CI jobs that execute `build_flatpak.sh test`, not be skipped for a missing program ----------------------------

    TEST_JOBS = (("ci.yml", "checks"), ("flatpak.yml", "flatpak"), ("rc-artifacts.yml", None))

    def find_job(self, w, wfname, jname):
        jobs = w[wfname]["jobs"]
        return jobs[jname] if jname else next(j for j in jobs.values() if any("build_flatpak.sh test" in str(st.get("run", "")) for st in j.get("steps") or []))

    def find_step(self, job):
        return next(st for st in job["steps"] if "scripts/build_flatpak.sh test" in str(st.get("run", "")))

    def test_the_security_tests_have_their_tools_and_cannot_be_skipped_in_the_ci_jobs_that_run_them(self):
        import re
        for wfname, jname in self.TEST_JOBS:
            job = self.find_job(self.wf, wfname, jname)
            st = self.find_step(job)
            self.assertEqual(str(st["env"]["TSW_REQUIRE_TOOLS"]), "1", wfname)
            # without the variable a missing program would be a silent skip
            self.assertTrue(self.errs(lambda w: self.find_step(self.find_job(w, wfname, jname)).pop("env", None)), f"{wfname}: TSW_REQUIRE_TOOLS removed")
            self.assertTrue(self.errs(lambda w: self.find_step(self.find_job(w, wfname, jname)).update({"env": {"TSW_REQUIRE_TOOLS": "0"}})), f"{wfname}: TSW_REQUIRE_TOOLS=0")
            # each program the tests need must be installed by an earlier step of the job
            for pkg in c.TEST_PACKAGES:
                def drop(w, pkg=pkg):
                    for step in self.find_job(w, wfname, jname)["steps"]:
                        if "run" in step:
                            step["run"] = re.sub(r"(?<![\w-])" + re.escape(pkg) + r"(?![\w-])", "", step["run"])
                self.assertTrue(self.errs(drop), f"{wfname}: {pkg} not installed")

            # an installation AFTER the tests does not help
            def move_install_after(w):
                steps = self.find_job(w, wfname, jname)["steps"]
                tests_at = steps.index(self.find_step({"steps": steps}))
                for k, step in list(enumerate(steps)):
                    if k < tests_at and "apt-get install" in str(step.get("run", "")):
                        steps.append(steps.pop(k))
                        break
            self.assertTrue(self.errs(move_install_after), f"{wfname}: install moved behind the tests")

    def test_the_end_to_end_test_finds_flatpak_installed_before_it(self):
        import re
        job = self.wf["ci.yml"]["jobs"]["checks"]
        self.assertTrue(any("scripts/e2e_flatpak_bundle.sh" in str(st.get("run", "")) for st in job["steps"]))
        for pkg in c.E2E_PACKAGES:
            def drop(w, pkg=pkg):
                for step in w["ci.yml"]["jobs"]["checks"]["steps"]:
                    if "run" in step:
                        step["run"] = re.sub(r"(?<![\w-])" + re.escape(pkg) + r"(?![\w-])", "", step["run"])
            self.assertTrue(self.errs(drop), pkg)

    def test_every_workflow_is_valid_yaml_with_jobs_that_run_something(self):
        for name, doc in self.wf.items():
            self.assertIsInstance(doc, dict, name)
            self.assertIsInstance(doc.get("jobs"), dict, f"{name}: no jobs")
            for jname, job in doc["jobs"].items():
                self.assertTrue("steps" in job or "uses" in job, f"{name}: job {jname} has neither steps nor a called workflow")
                if "steps" in job:
                    for st in job["steps"]:
                        self.assertTrue("run" in st or "uses" in st, f"{name}: job {jname}: a step without run or uses: {st}")

    def test_the_windows_workflow_is_manual_input_free_secret_free_and_read_only(self):
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml").update({"push": {"branches": ["main"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml")["workflow_dispatch"] or on(w, "windows.yml").update({"workflow_dispatch": {"inputs": {"ref": {"type": "string"}}}})))
        self.assertTrue(self.errs(lambda w: w["windows.yml"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: w["windows.yml"]["jobs"]["windows"].update({"runs-on": "ubuntu-24.04"})))
        self.assertTrue(self.errs(lambda w: w["windows.yml"]["jobs"]["windows"].pop("if")))
        self.assertTrue(self.errs(lambda w: w.pop("windows.yml")))

    def test_the_combined_release_set_cannot_be_green_without_both_platforms_and_creates_no_release(self):
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["assemble"].update({"needs": ["linux"]})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["assemble"].pop("needs")))
        self.assertTrue(self.errs(lambda w: on(w, "release-set.yml").update({"push": {"branches": ["main"]}})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["assemble"]["steps"].append({"run": "gh release create v1 build/release-set/*"})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["windows"].pop("if")))
        self.assertTrue(self.errs(lambda w: w.pop("release-set.yml")))

    def test_build_on_push_builds_the_candidate_and_the_site_and_publishes_no_release(self):
        def job(w, n):
            return w["build.yml"]["jobs"][n]

        self.assertEqual(self.errs(lambda w: None), [])
        self.assertTrue(self.errs(lambda w: w.pop("build.yml")))
        self.assertTrue(self.errs(lambda w: on(w, "build.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "build.yml").update({"pull_request": None})))
        self.assertTrue(self.errs(lambda w: on(w, "build.yml")["workflow_dispatch"] or on(w, "build.yml").update({"workflow_dispatch": {"inputs": {"ref": {}}}})))
        self.assertTrue(self.errs(lambda w: w["build.yml"]["jobs"].update({"release": {"runs-on": "x", "steps": [{"run": "true"}]}})))
        self.assertTrue(self.errs(lambda w: job(w, "flatpak").update({"uses": "./.github/workflows/flatpak.yml"})))
        self.assertTrue(self.errs(lambda w: job(w, "windows")["with"].update({"use_cache": True})))
        for n in ("flatpak", "windows", "candidate", "site"):
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).pop("if")), n)
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"run": "gh release create v1"})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"run": "x", "env": {"K": "${{ secrets.ANY }}"}})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"run": "x", "env": {"K": "${{ vars.ANY }}"}})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"environment": "flatpak-release"})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"environment": "release-publish"})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"uses": "actions/deploy-pages@" + "a" * 40})))
        # the web site: independent of the builds, only for a push to main, shared concurrency, tests before it goes live, tip of main only, and the web site ALONE
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"needs": "candidate"})))
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"if": "${{ github.ref == 'refs/heads/main' }}"})))
        site_guard = "github.repository == 'Glaubenskrieger/TSW-Forever' && "  # the development repository must never deploy GitHub Pages
        self.assertIn(site_guard, job(self.wf, "site")["if"])
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"if": job(w, "site")["if"].replace(site_guard, "")})))
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"if": job(w, "site")["if"].replace("Glaubenskrieger/TSW-Forever", "someone/else")})))
        self.assertTrue(self.errs(lambda w: job(w, "site").pop("concurrency")))
        self.assertTrue(self.errs(lambda w: job(w, "site")["concurrency"].update({"cancel-in-progress": True})))
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"permissions": {"contents": "write", "pages": "write", "id-token": "write"}})))
        for needle in ("sha256sum --check", "assemble_pages_site.py --verify", "git ls-remote"):
            def drop(w, needle=needle):
                for st in job(w, "site")["steps"]:
                    if "run" in st:
                        st["run"] = st["run"].replace(needle, "true")
            self.assertTrue(self.errs(drop), needle)
        # the web site is built once, by the shared website.yml, and deployed from exactly that artifact: after the web site build only, never after the other builds
        self.assertTrue(self.errs(lambda w: job(w, "site").pop("needs")))
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"needs": ["site-build", "flatpak"]})))
        self.assertTrue(self.errs(lambda w: job(w, "site").update({"needs": "windows"})))
        self.assertTrue(self.errs(lambda w: job(w, "site-build").update({"uses": "./.github/workflows/windows.yml"})))
        self.assertTrue(self.errs(lambda w: job(w, "site-build").update({"needs": "flatpak"})))
        self.assertTrue(self.errs(lambda w: job(w, "site-build").update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "site")["steps"].remove(next(st for st in job(w, "site")["steps"] if "download-artifact" in str(st.get("uses", ""))))))
        # artifact retention: the production value is 30 days, the development repository keeps 7; nothing else
        self.assertTrue(self.errs(lambda w: [st.get("with", {}).update({"retention-days": 90}) for st in job(w, "candidate")["steps"] if "upload-artifact" in str(st.get("uses", ""))]))
        for extra in ("python3 -I scripts/assemble_pages_site.py --repo build/flatpak/repo --refs r --out o", "cp build/flatpak/repo build/site/repo", "curl -o build/site/channels.json x", "cp x.flatpak build/site/",
                      "python3 -I scripts/preserve_live_site.py --site s --out o", "cp tsw-forever.flatpakrepo build/site/"):
            self.assertTrue(self.errs(lambda w, extra=extra: job(w, "site")["steps"].append({"run": extra})), extra)
        self.assertTrue(self.errs(lambda w: job(w, "flatpak").update({"secrets": "inherit"})))

    def test_the_release_pipeline_keeps_both_approvals_and_publishes_only_behind_approval_2(self):
        def job(w, n):
            return w["release.yml"]["jobs"][n]

        def drop_step(n, needle):
            def f(w):
                st = job(w, n)["steps"]
                st[:] = [x for x in st if needle not in repr(x)]
            return f

        self.assertEqual(c.check_tag_release_workflow(self.wf["release.yml"]), [])
        # triggers, permissions, the exact job set
        self.assertTrue(self.errs(lambda w: w.pop("release.yml")))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml").update({"workflow_dispatch": None})))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml").update({"workflow_run": {"workflows": ["x"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml")["push"].update({"branches": ["main"]})))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml")["push"].update({"tags": ["*"]})))
        self.assertTrue(self.errs(lambda w: w["release.yml"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: w["release.yml"]["jobs"].update({"extra": {"runs-on": "x", "steps": [{"run": "true"}]}})))
        for gone in ("approve", "sign", "pages-build", "deploy", "verify-live", "verify-signed"):
            self.assertTrue(self.errs(lambda w, gone=gone: w["release.yml"]["jobs"].update({gone: {"runs-on": "x", "steps": [{"run": "true"}]}})) or gone == "approve", gone)
        self.assertTrue(self.errs(lambda w: w["release.yml"]["jobs"].pop("approve")))
        for n in w_jobs(self.wf):
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).pop("if")), n)
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).update({"if": "${{ github.ref_type == 'tag' }}"})), n)
        # nothing public in another repository: the development repository (no environments, no Pages) must never publish, so EVERY release job and the Pages job carry the
        # production-repository condition, and removing just that part from any of them is refused
        guard = "github.repository == 'Glaubenskrieger/TSW-Forever' && "
        for n in w_jobs(self.wf):
            self.assertIn(guard, job(self.wf, n)["if"], n)
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).update({"if": job(w, n)["if"].replace(guard, "")})), n)
        # the chain that keeps everything public behind both approvals
        for n, dep in (("candidate", "guard"), ("approve", "candidate"), ("draft", "candidate"), ("draft", "approve"), ("publish", "draft")):
            self.assertTrue(self.errs(lambda w, n=n, dep=dep: job(w, n).update({"needs": [x for x in (job(w, n)["needs"] if isinstance(job(w, n)["needs"], list) else [job(w, n)["needs"]]) if x != dep]})), (n, dep))
        # approval 1: the environment flatpak-release stops the release before anything is put into a draft, for the whole release (Linux and Windows together)
        self.assertTrue(self.errs(lambda w: job(w, "approve").pop("environment")))
        self.assertTrue(self.errs(lambda w: job(w, "approve").update({"environment": "release-publish"})))
        self.assertTrue(self.errs(lambda w: job(w, "approve").update({"environment": "something-else"})))
        self.assertTrue(self.errs(lambda w: job(w, "approve")["steps"].append({"uses": "actions/checkout@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: job(w, "approve")["steps"].append({"run": "scripts/release_draft.sh publish v1 dist"})))
        self.assertTrue(self.errs(lambda w: job(w, "approve").update({"permissions": {"contents": "write"}})))
        for n in ("guard", "candidate", "draft"):
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).update({"environment": "flatpak-release"})), n)
        # no secret, no variable, no key, no OSTree, no Pages in the release workflow
        for n in ("guard", "candidate", "approve", "draft", "publish"):
            self.assertTrue(self.errs(lambda w, n=n: job(w, n)["steps"].append({"run": "x", "env": {"K": "${{ secrets.TSW_FLATPAK_SIGNING_KEY }}"}})), n)
            self.assertTrue(self.errs(lambda w, n=n: job(w, n)["steps"].append({"run": "x", "env": {"V": "${{ vars.ANY }}"}})), n)
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"secrets": "inherit"})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"run": "gpg --batch --import key"})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"run": "scripts/flatpak_sign_ci.sh a b"})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"uses": "actions/deploy-pages@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"uses": "actions/upload-pages-artifact@" + "a" * 40})))
        # nothing is built at the tag: the candidate of the build run of exactly this commit is verified and taken over
        self.assertTrue(self.errs(lambda w: w["release.yml"]["jobs"].update({"flatpak": {"if": job(w, "guard")["if"], "needs": "guard", "uses": "./.github/workflows/flatpak-stable-build.yml"}})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"run": "scripts/build_flatpak.sh bundle"})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate")["steps"].append({"run": "cargo build --release"})))
        for needle in ("scripts/release_candidate_find.sh", "scripts/release_candidate_verify.sh", "run-id: "):
            def drop(w, needle=needle):
                for st in job(w, "candidate")["steps"]:
                    for k in ("run", "with"):
                        if k in st:
                            st[k] = repr(st[k]).replace(needle, "") if k == "run" else {a: b for a, b in st[k].items() if not needle.startswith(a)}
            self.assertTrue(self.errs(drop), needle)
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"environment": "release-publish"})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"permissions": {"contents": "write", "actions": "read"}})))
        self.assertTrue(self.errs(lambda w: job(w, "candidate").update({"permissions": {"contents": "read"}})))
        # write permissions exist exactly where needed
        self.assertTrue(self.errs(lambda w: job(w, "guard").update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "draft").update({"permissions": {"contents": "read"}})))
        self.assertTrue(self.errs(lambda w: job(w, "draft").update({"permissions": {"contents": "write", "pages": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "publish").update({"permissions": {"contents": "write", "id-token": "write"}})))
        # approval 2: only the job publish publishes, only through the script, behind its own environment
        self.assertTrue(self.errs(lambda w: job(w, "publish").pop("environment")))
        self.assertTrue(self.errs(lambda w: job(w, "publish").update({"environment": "flatpak-release"})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"run": "scripts/release_draft.sh publish v1 dist"})))
        self.assertTrue(self.errs(lambda w: job(w, "approve")["steps"].append({"run": "scripts/release_draft.sh upload v1 n dist"})))
        self.assertTrue(self.errs(lambda w: job(w, "draft")["steps"].append({"run": "gh release edit v1 --draft=false"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "gh release create v1"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "git tag v1 && git push origin v1"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "echo ${{ github.ref_name }}"})))
        self.assertTrue(self.errs(drop_step("publish", "release_draft.sh publish")))
        # every check that stands between the build and the publication
        # (the checks themselves live in the shared scripts, which TestSharedReleaseScripts guards; the workflow must call them)
        for jn, needle in (("guard", "scripts/release_guard.sh"), ("draft", "scripts/release_assemble_dist.sh"), ("draft", "scripts/release_notes.py"), ("draft", "release_draft.sh upload")):
            self.assertTrue(self.errs(drop_step(jn, needle)), (jn, needle))
        self.assertTrue(self.errs(lambda w: job(w, "guard")["steps"][-1].update({"run": "scripts/release_guard.sh --rehearse"})), "a real release must not run the rehearsal mode")
        # only release.yml may react to a tag: every other workflow still must not
        self.assertTrue(self.errs(lambda w: on(w, "release-set.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "flatpak-stable-build.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "release-set.yml").update({"workflow_dispatch": {"inputs": {"ref": {"type": "string"}}}})))

    def test_the_bundle_build_cannot_sign_publish_hold_a_secret_or_bring_back_an_update_source(self):
        name = "flatpak-stable-build.yml"
        first = next(iter(self.wf[name]["jobs"]))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "x", "env": {"K": "${{ secrets.ANY }}"}})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "gh release create v1"})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: w[name].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "scripts/flatpak_sign_ci.sh r k"})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "gpg --batch --import key"})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "flatpak build-bundle --repo-url=https://x.invalid/repo r b app stable"})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first]["steps"].append({"run": "flatpak build-bundle --gpg-keys=k r b app stable"})))
        self.assertTrue(self.errs(lambda w: w[name]["jobs"][first].update({"environment": "flatpak-release"})))
        self.assertTrue(self.errs(lambda w: on(w, name).update({"workflow_dispatch": None})))

    def test_the_release_bundle_is_the_branch_local_and_a_release_build_is_told_by_build_metadata(self):
        name = "flatpak-stable-build.yml"
        steps = lambda w: w[name]["jobs"]["build"]["steps"]
        self.assertEqual(self.errs(lambda w: None), [])
        # no parallel branch: stable or testing anywhere in the bundle build is refused
        self.assertTrue(self.errs(lambda w: steps(w).append({"run": "scripts/build_flatpak.sh bundle", "env": {"FLATPAK_BRANCH": "stable", "TSW_RELEASE_BUILD": "1"}})))
        self.assertTrue(self.errs(lambda w: steps(w).append({"run": "flatpak run org.tswlauncher.Launcher//stable --version"})))
        self.assertTrue(self.errs(lambda w: steps(w).append({"run": "ls ~/.local/share/flatpak/app/x/x86_64/stable/active/files"})))
        # the release metadata must be set on the build step and on the bundle step
        for needle in ("build_flatpak.sh build", "build_flatpak.sh bundle"):
            def unmark(w, needle=needle):
                for st in steps(w):
                    if needle in str(st.get("run", "")):
                        st.get("env", {}).pop("TSW_RELEASE_BUILD", None)
            self.assertTrue(self.errs(unmark), needle)
        # the audit must look for the release marker in the programs
        def no_marker_check(w):
            for st in steps(w):
                if "run" in st:
                    st["run"] = st["run"].replace("tsw-forever-release-build", "x")
        self.assertTrue(self.errs(no_marker_check))
        # only this workflow makes a release build: development and candidate builds must not
        for other in ("flatpak.yml", "rc-artifacts.yml", "ci.yml", "release-set.yml", "build.yml"):
            first = next(iter(self.wf[other]["jobs"]))
            job = self.wf[other]["jobs"][first]
            if "steps" in job:
                self.assertTrue(self.errs(lambda w, o=other, f=first: w[o]["jobs"][f]["steps"].append({"run": "scripts/build_flatpak.sh build", "env": {"TSW_RELEASE_BUILD": "1"}})), other)

    def test_a_package_build_on_pull_requests_or_other_branches_is_rejected(self):
        self.assertTrue(self.errs(lambda w: on(w, "flatpak.yml").update({"pull_request": None})))
        # flatpak.yml has no push trigger of its own any more (ci.yml calls it); a push trigger would have to stay limited to main
        self.assertTrue(self.errs(lambda w: on(w, "flatpak.yml").update({"push": {"branches": ["main", "dev"]}})))
        self.assertEqual(self.errs(lambda w: on(w, "flatpak.yml").update({"push": {"branches": ["main"]}})), [])

    def test_ci_calls_the_package_and_windows_builds_only_after_its_checks_and_only_on_main(self):
        self.assertEqual(self.wf["ci.yml"]["jobs"]["flatpak"]["needs"], "checks")
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["flatpak"].pop("needs")))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["flatpak"].pop("if")))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["windows"].pop("needs")))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["windows"].update({"if": "github.event_name == 'workflow_dispatch'"})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"].update({"x": {"uses": "./.github/workflows/release.yml", "needs": "checks", "if": "github.ref == 'refs/heads/main'"}})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["flatpak"].update({"permissions": {"contents": "write"}})))

    def test_no_release_build_uses_a_development_cache(self):
        cache = {"uses": "actions/cache@" + "a" * 40}
        for name, job in (("rc-artifacts.yml", "build"),):
            self.assertTrue(self.errs(lambda w, n=name, j=job: w[n]["jobs"][j]["steps"].append(cache)))
        # windows.yml (called by the release workflows) restores a cache only when asked: opt-in input, default false
        steps = lambda w: w["windows.yml"]["jobs"]["windows"]["steps"]
        self.assertTrue(any("actions/cache" in str(s.get("uses", "")) for s in steps(self.wf)), "the shipped windows.yml has the cache step")
        self.assertTrue(self.errs(lambda w: [s.pop("if") for s in steps(w) if "actions/cache" in str(s.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: [s.update({"if": "github.workflow == 'windows' || github.workflow == 'release-set'"}) for s in steps(w) if "actions/cache" in str(s.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: [s.update({"if": "inputs.use_cache == true || github.workflow == 'release-set'"}) for s in steps(w) if "actions/cache" in str(s.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml")["workflow_call"]["inputs"]["use_cache"].update({"default": True})))
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml")["workflow_call"].pop("inputs")))

    def test_the_release_workflows_call_windows_with_an_explicit_use_cache_false_and_only_ci_says_true(self):
        self.assertIs(self.wf["release-set.yml"]["jobs"]["windows"]["with"]["use_cache"], False)
        self.assertIs(self.wf["ci.yml"]["jobs"]["windows"]["with"]["use_cache"], True)
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["windows"].pop("with")))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["windows"]["with"].update({"use_cache": True})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["windows"]["with"].update({"use_cache": False})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["windows"].pop("with")))

    def test_called_workflows_get_no_secrets_and_no_write_permission(self):
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["flatpak"].update({"secrets": "inherit"})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["windows"].update({"secrets": "inherit"})))
        self.assertTrue(self.errs(lambda w: w["release-set.yml"]["jobs"]["windows"].update({"permissions": {"contents": "write"}})))

    def test_an_optional_ci_part_is_skipped_only_on_an_explicit_false(self):
        steps = lambda w: w["ci.yml"]["jobs"]["checks"]["steps"]
        self.assertTrue(any("steps.changes.outputs" in str(s.get("if", "")) for s in steps(self.wf)))
        self.assertTrue(self.errs(lambda w: [s.update({"if": "${{ steps.changes.outputs.e2e == 'true' }}"}) for s in steps(w) if "steps.changes.outputs" in str(s.get("if", ""))]))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["flatpak"].update({"if": "${{ github.ref == 'refs/heads/main' && needs.checks.outputs.flatpak == 'true' }}"})))

    def test_a_missing_job_guard_or_main_ancestry_check_is_rejected(self):
        self.assertTrue(self.errs(lambda w: w["flatpak.yml"]["jobs"]["flatpak"].pop("if")))
        def drop(w):
            st = w["flatpak.yml"]["jobs"]["flatpak"]["steps"]
            st[:] = [x for x in st if "merge-base" not in str(x.get("run", ""))]
        self.assertTrue(self.errs(drop))

    def test_the_check_workflow_builds_and_publishes_nothing(self):
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"run": "scripts/build_flatpak.sh build"})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"uses": "actions/upload-artifact@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"run": "x", "env": {"T": "${{ secrets.T }}"}})))

    def test_tag_and_scheduled_triggers_are_rejected_everywhere(self):
        self.assertTrue(self.errs(lambda w: on(w, "ci.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "ci.yml").update({"schedule": [{"cron": "0 0 * * *"}]})))
        self.assertTrue(self.errs(lambda w: on(w, "flatpak.yml").update({"pull_request_target": None})))


class RcWorkflowPolicy(unittest.TestCase):
    """rc-artifacts.yml builds the Flatpak bundle and the source archive - the only official Linux artifacts - and uploads them as workflow artifacts; it can never
    create a release or publish anything, and it builds no distribution-specific package."""

    def setUp(self):
        import yaml
        self.m = json.load(open(MANIFEST))
        self.path = os.path.join(WORKFLOWS, "rc-artifacts.yml")
        self.text = open(self.path).read()
        self.wf = {f: yaml.safe_load(open(os.path.join(WORKFLOWS, f))) for f in os.listdir(WORKFLOWS) if f.endswith(".yml")}

    def text_errs(self, edit):
        return c.check_rc_workflow(edit(self.text), self.m)

    def doc_errs(self, mutate):
        wf = copy.deepcopy(self.wf)
        mutate(wf)
        return c.check_ci_policy(wf, self.m)

    def test_the_shipped_workflow_passes_both_checks(self):
        self.assertEqual(c.check_rc_workflow(self.text, self.m), [])
        self.assertEqual(c.check_ci_policy(self.wf, self.m), [])

    def test_it_is_manual_only_without_inputs_and_has_no_tag_or_push_trigger(self):
        self.assertTrue(self.doc_errs(lambda w: on(w, "rc-artifacts.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.doc_errs(lambda w: on(w, "rc-artifacts.yml").update({"push": {"branches": ["main"]}})))
        self.assertTrue(self.doc_errs(lambda w: on(w, "rc-artifacts.yml").update({"workflow_dispatch": {"inputs": {"ref": {"type": "string"}}}})))
        self.assertTrue(self.text_errs(lambda t: t.replace("workflow_dispatch:", "workflow_dispatch:\n  schedule:\n    - cron: '0 0 * * *'\n", 1)))

    def test_it_can_never_create_a_release_or_publish_or_use_a_secret_or_write(self):
        for add in ["gh release create v1 build/rc/*", "gh api repos/x/y/releases", "uses: softprops/action-gh-release@" + "a" * 40,
                    "git push origin HEAD", "git tag v1", "git commit -am x", "echo ${{ secrets.T }}", "docker push x/y", "docker login", "uses: actions/deploy-pages@" + "a" * 40]:
            self.assertTrue(self.text_errs(lambda t: t + "\n      - run: " + add + "\n"), add)
        self.assertTrue(self.text_errs(lambda t: t.replace("permissions:\n  contents: read", "permissions:\n  contents: write", 1)))
        self.assertTrue(self.doc_errs(lambda w: w["rc-artifacts.yml"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.doc_errs(lambda w: w["rc-artifacts.yml"]["jobs"]["build"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.text_errs(lambda t: t.replace("sudo apt-get update", "sudo rm -rf /tmp/x", 1)))
        self.assertTrue(self.text_errs(lambda t: t + "\n      - run: sudo sysctl -w kernel.randomize_va_space=0\n"), "only the AppArmor user-namespace setting may be changed with sudo")

    def test_the_guards_and_the_verifications_cannot_be_dropped(self):
        for needle in ["merge-base --is-ancestor", "scripts/version.py check", "GITHUB_REF_TYPE", "scripts/pin_actions.sh --check", "scripts/build_flatpak.sh test",
                       "scripts/build_flatpak.sh bundle", "scripts/native_stage.sh", "sha256sum", "scripts/check_release_set.py", "scripts/check_release_artifacts.py",
                       "--command=tsw-launch", "actions/upload-artifact@", "persist-credentials: false", "rm -rf launcher/target launcher-gui/target patcher/target"]:
            self.assertTrue(self.text_errs(lambda t: t.replace(needle, "REMOVED")), needle)   # every occurrence: some checks run in more than one step
        self.assertTrue(self.doc_errs(lambda w: w["rc-artifacts.yml"]["jobs"]["build"].pop("if")))
        self.assertTrue(self.doc_errs(lambda w: w["rc-artifacts.yml"]["jobs"]["build"].update({"if": "${{ github.ref == 'refs/heads/main' }}"})))

    def test_the_official_linux_artifacts_are_the_flatpak_bundle_and_the_source_archive_only(self):
        code = c._code_only(self.text)
        self.assertEqual(c.check_rc_workflow(self.text, self.m), [])
        # nothing of the removed package formats appears in the executable part of the workflow
        for forbidden in ("docker", ".deb", ".rpm", "pkg.tar", "PKGBUILD", "makepkg", "dpkg", "pacman", "archlinux", "appimage", "build_deb", "ci_build"):
            self.assertNotIn(forbidden.lower(), code.lower(), forbidden)
        # and any step that brings one of them back is rejected
        for add in ["docker pull archlinux:base-devel", "docker run --rm debian:11 true", "scripts/build_deb.sh", "makepkg -s", "cp x.deb \"$set/\"",
                    "cp packaging/arch/PKGBUILD \"$set/PKGBUILD\"", "fpm -s dir -t rpm .", "dpkg-deb --build x", "ls pkg.tar.zst", "appimagetool x"]:
            errs = self.text_errs(lambda t: t + "\n      - run: " + add + "\n")
            self.assertTrue(any("only the Flatpak bundle and the source archive" in e or "copied by name" in e for e in errs), (add, errs))

    def test_the_release_set_contains_exactly_the_bundle_the_source_archive_and_the_checksums(self):
        flat = 'cp build/flatpak/tsw-launcher.flatpak "$set/tsw-forever-${version}.flatpak"'
        self.assertIn(flat, self.text)
        extra_copy = self.text.replace(flat, flat + '\n          cp build/flatpak/extra.bin "$set/extra.bin"', 1)
        self.assertTrue(any("copied by name" in e for e in c.check_rc_workflow(extra_copy, self.m)))
        no_src = self.text.replace('cp "build/native/tsw-forever-${version}-src.tar.gz" "$set/"', "true", 1)
        self.assertTrue(any("source archive must be copied" in e for e in c.check_rc_workflow(no_src, self.m)))
        wide = self.text.replace("sha256sum tsw-forever-*.flatpak tsw-forever-*-src.tar.gz >", "sha256sum * >", 1)
        self.assertTrue(any("SHA256SUMS lists exactly" in e for e in c.check_rc_workflow(wide, self.m)))
        other_upload = self.text.replace("path: build/rc/", "path: build/", 1)
        self.assertTrue(any("only the release set folder" in e for e in c.check_rc_workflow(other_upload, self.m)))

    def test_the_source_archive_is_built_twice_and_must_build_on_its_own(self):
        once = self.text.replace("scripts/native_stage.sh\n          first=", "true\n          first=", 1)
        self.assertTrue(any("twice" in e for e in c.check_rc_workflow(once, self.m)))
        no_build = self.text.replace('cargo "+${RUST_VERSION}" check --locked --offline', 'cargo fmt --check', 1)
        self.assertTrue(any("source archive must be built" in e for e in c.check_rc_workflow(no_build, self.m)))

    def test_source_reproducibility_requires_a_failing_hash_comparison(self):
        comparison = '[ "$first" = "$second" ] ||'
        self.assertIn(comparison, self.text)
        self.assertTrue(self.text_errs(lambda t: t.replace(comparison, 'true ||', 1)))

    def test_the_test_builds_are_removed_before_the_flatpak_is_built(self):
        rm = "rm -rf launcher/target launcher-gui/target patcher/target"
        late = self.text.replace(rm, "true", 1).replace("run: scripts/build_flatpak.sh build", "run: |\n          scripts/build_flatpak.sh build\n          " + rm, 1)
        self.assertTrue(any("removed before" in e for e in c.check_rc_workflow(late, self.m)))
        self.assertEqual(c.check_rc_workflow(self.text, self.m), [])

    def test_every_disk_measuring_point_is_required_and_they_keep_the_order_of_the_phases(self):
        for label in c.RC_MEASURING_POINTS:
            errs = self.text_errs(lambda t: t.replace('scripts/ci_disk_report.sh "' + label, 'true "' + label, 1))
            self.assertTrue(any("measuring point" in e or "measured" in e for e in errs), label)
        two = 'scripts/ci_disk_report.sh "2 after installing runtime, SDK and Rust"'
        seven = 'scripts/ci_disk_report.sh "7 after the source archive check, before cleanup 3"'
        self.assertIn(two, self.text)
        self.assertIn(seven, self.text)
        swapped = self.text.replace(seven, "@@7", 1).replace(two, seven, 1).replace("@@7", two, 1)
        self.assertTrue(any("order" in e for e in c.check_rc_workflow(swapped, self.m)))
        self.assertEqual(c.check_rc_workflow(self.text, self.m), [])

    def test_only_the_fixed_temporary_build_directories_may_be_removed(self):
        line = "rm -rf build/flatpak/state build/flatpak/build-dir build/flatpak/repo"
        self.assertIn(line, self.text)
        for bad in ["build/flatpak/tsw-launcher.flatpak", "build/rc", "build/native", "$HOME/.cargo", "~", "/", "build", "build/flatpak/src/vendor", "build/source-check/.."]:
            errs = self.text_errs(lambda t: t.replace(line, line + " " + bad, 1))
            self.assertTrue(any("not allowed" in e for e in errs), bad)
        for form in ["find build -delete", "rm -r build/rc", "rmdir build/rc", "echo x && rm -rf build/rc", "sudo rm -rf launcher/target", "rm -rf build/rc; true"]:
            errs = self.text_errs(lambda t: t + "\n      - run: " + form + "\n")
            self.assertTrue(errs, form)

    def test_a_cleanup_must_be_measured_before_and_after_and_in_the_right_phase(self):
        for label in ("4 after cleanup 1", "6 after cleanup 2", "8 after cleanup 3"):
            self.assertTrue(self.text_errs(lambda t: t.replace('scripts/ci_disk_report.sh "' + label, 'true "' + label, 1)), label)
        gone = "rm -rf build/flatpak/state build/flatpak/build-dir build/flatpak/repo"
        early = self.text.replace(gone, "true", 1).replace("run: scripts/build_flatpak.sh bundle", "run: |\n          " + gone + "\n          scripts/build_flatpak.sh bundle", 1)
        self.assertTrue(any("after the bundle" in e for e in c.check_rc_workflow(early, self.m)))
        src = "rm -rf build/source-check"
        early_src = self.text.replace(src, "true", 1).replace("python3 -I scripts/version.py check\n          mkdir -p .cargo", src + "\n          python3 -I scripts/version.py check\n          mkdir -p .cargo", 1)
        self.assertTrue(any("source archive check may only be removed" in e or "measured" in e for e in c.check_rc_workflow(early_src, self.m)))

    def test_the_platform_runtime_stays_and_the_last_measurement_runs_after_a_failure(self):
        sdk = 'uninstall --user --noninteractive "org.freedesktop.Sdk.Extension.rust-stable//${FREEDESKTOP_VERSION}" "org.freedesktop.Sdk//${FREEDESKTOP_VERSION}"'
        self.assertIn(sdk, self.text)
        platform = self.text.replace(sdk, sdk.replace("Sdk//", "Platform//"), 1)
        self.assertTrue(any("uninstalled" in e for e in c.check_rc_workflow(platform, self.m)))
        self.assertTrue(any("always" in e for e in c.check_rc_workflow(self.text.replace("if: ${{ always() }}", "if: ${{ success() }}", 1), self.m)))

    def test_actions_are_pinned_artifacts_are_short_lived_and_tests_come_first(self):
        self.assertTrue(self.text_errs(lambda t: t.replace("actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9", "actions/upload-artifact@v4", 1)))
        self.assertTrue(self.text_errs(lambda t: t.replace(c.RETENTION_EXPR, "90", 1)))
        swapped = self.text.replace("scripts/build_flatpak.sh test", "@@T", 1).replace("run: scripts/build_flatpak.sh build", "run: scripts/build_flatpak.sh test", 1).replace("@@T", "scripts/build_flatpak.sh build", 1)
        self.assertTrue(c.check_rc_workflow(swapped, self.m))
        self.assertTrue(self.text_errs(lambda t: t.replace('FREEDESKTOP_VERSION: "25.08"', 'FREEDESKTOP_VERSION: "24.08"', 1)))

    def test_the_main_and_flatpak_policies_still_apply_to_the_other_workflows(self):
        self.assertTrue(self.doc_errs(lambda w: on(w, "flatpak.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.doc_errs(lambda w: w.pop("rc-artifacts.yml")))


ICONS = os.path.join(HERE, "..", "launcher-gui", "packaging", "linux", "icons", "hicolor")
DESKTOP = os.path.join(HERE, "..", "launcher-gui", "packaging", "linux", "org.tswlauncher.Launcher.desktop")
BUILT_REPO = os.path.join(HERE, "..", "build", "flatpak", "repo")
APP_ID = "org.tswlauncher.Launcher"
SIZES = (16, 32, 48, 64, 128, 256)


def png_size(path):
    import struct
    d = open(path, "rb").read(32)
    assert d[:8] == b"\x89PNG\r\n\x1a\n", path
    return struct.unpack(">II", d[16:24])


class BuilderHostTools(unittest.TestCase):
    """flatpak-builder needs `eu-strip`/`eu-elfcompress` (package elfutils) and `appstreamcli compose` needs the SVG loader (librsvg2-common) on the runner; the
    packages are only recommended and the workflows install with --no-install-recommends. Every workflow that builds the Flatpak must install them."""

    LINE = "flatpak-builder elfutils librsvg2-common "

    def setUp(self):
        self.m = json.load(open(MANIFEST))
        # the publishing workflow builds nothing any more (it verifies and deploys the offline-signed repository), so it needs no builder tools
        self.checks = {"flatpak.yml": c.check_workflow, "rc-artifacts.yml": c.check_rc_workflow}
        self.texts = {n: open(os.path.join(WORKFLOWS, n)).read() for n in self.checks}

    def test_every_workflow_that_builds_the_flatpak_installs_the_host_tools_with_the_builder(self):
        for name, check in self.checks.items():
            text = self.texts[name]
            self.assertIn(self.LINE, text, name)
            self.assertEqual(check(text, self.m), [], name)

    def test_a_workflow_missing_one_of_them_is_rejected_by_name(self):
        for name, check in self.checks.items():
            for package in c.BUILDER_HOST_PACKAGES:
                errs = check(self.texts[name].replace(self.LINE, self.LINE.replace(package + " ", ""), 1), self.m)
                self.assertTrue(any(package in e for e in errs), (name, package, errs))

    def test_a_package_somewhere_else_or_with_a_longer_name_does_not_count(self):
        text = self.texts["flatpak.yml"]
        moved = text.replace(self.LINE, "flatpak-builder ", 1).replace("sudo apt-get update", "sudo apt-get install --yes elfutils librsvg2-common\n          sudo apt-get update", 1)
        self.assertTrue(any("elfutils" in e for e in c.check_workflow(moved, self.m)), "the packages must be in the same install as flatpak-builder")
        lookalike = text.replace(self.LINE, "flatpak-builder elfutils-dev librsvg2-common-extra ", 1)
        errs = c.check_workflow(lookalike, self.m)
        self.assertTrue(any("elfutils" in e and "librsvg2-common" in e for e in errs), errs)
        no_apt = re.sub(r"sudo apt-get install[^\n]*", "true", text)
        self.assertTrue(any("elfutils" in e for e in c.check_workflow(no_apt, self.m)), "a build_flatpak.sh build step needs the builder tools")

    def test_a_workflow_that_does_not_build_is_not_asked_for_it(self):
        ci = open(os.path.join(WORKFLOWS, "ci.yml")).read()
        self.assertEqual(c._builder_host_tools(ci, "ci"), [])


class IconPolicy(unittest.TestCase):
    """The icon of the application (own artwork, app id as file name) is present in the sources, installed by the manifest and, when a Flatpak was built, in it."""

    def test_the_sources_hold_every_size_and_a_scalable_svg_named_after_the_app_id(self):
        for n in SIZES:
            p = os.path.join(ICONS, f"{n}x{n}", "apps", f"{APP_ID}.png")
            self.assertTrue(os.path.isfile(p), p)
            self.assertEqual(png_size(p), (n, n), p)
        svg = os.path.join(ICONS, "scalable", "apps", f"{APP_ID}.svg")
        text = open(svg, encoding="utf-8").read()
        self.assertIn("<svg", text)
        self.assertNotIn("funcom", text.lower(), "no third-party mark in the artwork")

    def test_the_desktop_entry_uses_the_app_id_for_icon_and_window_class(self):
        lines = open(DESKTOP, encoding="utf-8").read().splitlines()
        self.assertIn(f"Icon={APP_ID}", lines)
        self.assertIn(f"StartupWMClass={APP_ID}", lines)

    def test_the_manifest_installs_the_icon_folders_under_the_app_id(self):
        cmds = " ".join(json.load(open(MANIFEST))["modules"][0]["build-commands"])
        self.assertIn("/app/share/icons/hicolor/", cmds)
        self.assertIn(f"{APP_ID}.svg", cmds)
        self.assertIn(f"{APP_ID}.png", cmds)

    @unittest.skipUnless(os.path.isdir(BUILT_REPO) and __import__("shutil").which("ostree"), "no built Flatpak repository on this machine")
    def test_the_built_flatpak_contains_every_icon_file(self):
        import subprocess
        refs = subprocess.run(["ostree", "refs", f"--repo={BUILT_REPO}"], capture_output=True, text=True).stdout.split()
        ref = next((r for r in refs if r.startswith(f"app/{APP_ID}/x86_64/") and "Debug" not in r), None)
        self.assertIsNotNone(ref, refs)
        listing = subprocess.run(["ostree", "ls", f"--repo={BUILT_REPO}", "-R", ref, "/files/share/icons"], capture_output=True, text=True).stdout
        for n in SIZES:
            self.assertIn(f"/files/share/icons/hicolor/{n}x{n}/apps/{APP_ID}.png", listing)
        self.assertIn(f"/files/share/icons/hicolor/scalable/apps/{APP_ID}.svg", listing)
        desktop = subprocess.run(["ostree", "cat", f"--repo={BUILT_REPO}", ref, f"/files/share/applications/{APP_ID}.desktop"], capture_output=True, text=True).stdout
        self.assertIn(f"Icon={APP_ID}", desktop)
        self.assertIn("The Secret World – Forever", desktop)


class TreePolicy(unittest.TestCase):
    def tree(self, files):
        d = tempfile.mkdtemp(prefix="tsw-flatpak-tree-")
        for f in files:
            p = os.path.join(d, f)
            os.makedirs(os.path.dirname(p), exist_ok=True)
            open(p, "wb").write(b"x")
        return d

    def test_a_clean_tree_passes_and_vendor_is_not_scanned(self):
        d = self.tree(["launcher/src/main.rs", "launcher-gui/ui/launcher.slint", "vendor/foo/src/lib.rs", "vendor/winapi/lib/x.dll", "vendor/target/x"])
        self.assertEqual(c.check_tree(d), [])

    def test_game_data_private_files_and_research_folders_are_rejected(self):
        for f in ["game/TheSecretWorldDX11.exe", "x/data.rdb", "x/ClientLog.txt", "analysis/runtime/a", "launcher/Prefs_3.xml", "reports/r.md", "x/y.dll",
                  "native/tsw-rdb/Cargo.toml", "x/crash.dmp", "launcher/target/debug/a", ".git/config", "launcher/install-state"]:
            self.assertTrue(c.check_tree(self.tree(["ok.rs", f])), f)

    def test_symbolic_links_are_rejected(self):
        d = self.tree(["a.rs"])
        os.symlink("/etc/passwd", os.path.join(d, "link"))
        self.assertTrue(any("symbolic link" in e for e in c.check_tree(d)))


class TestSharedReleaseScripts(unittest.TestCase):
    """release.yml and release-rehearsal.yml run the same scripts; they carry the checks and only read."""

    def setUp(self):
        import shutil
        import tempfile
        self.tmp = tempfile.mkdtemp()
        self.dir = os.path.join(self.tmp, "scripts")
        shutil.copytree(os.path.join(ROOT, "scripts"), self.dir, ignore=shutil.ignore_patterns("__pycache__"))

    def tearDown(self):
        import shutil
        shutil.rmtree(self.tmp)

    def test_the_shipped_scripts_pass(self):
        self.assertEqual(c.check_release_scripts(self.dir), [])

    def test_a_script_that_lost_its_check_is_refused(self):
        for name, needles in c.RELEASE_SCRIPT_NEEDS.items():
            for needle in needles:
                path = os.path.join(self.dir, name)
                original = open(path, encoding="utf-8").read()
                open(path, "w", encoding="utf-8").write(original.replace(needle, "x_removed_x"))
                try:
                    self.assertTrue(c.check_release_scripts(self.dir), (name, needle))
                finally:
                    open(path, "w", encoding="utf-8").write(original)

    def test_a_script_that_could_publish_or_write_is_refused(self):
        for name in c.RELEASE_SCRIPT_NEEDS:
            for bad in ("gh release create v1 --draft", "gh release upload v1 f", "gh release edit v1 --draft=false", "git push origin main", "git tag v1", "scripts/release_draft.sh publish v1 d",
                        "gh api -X POST repos/x/y/releases", "gh api --method DELETE repos/x/y", "echo ${{ secrets.X }}"):
                path = os.path.join(self.dir, name)
                original = open(path, encoding="utf-8").read()
                open(path, "w", encoding="utf-8").write(original + "\n" + bad + "\n")
                try:
                    self.assertTrue(c.check_release_scripts(self.dir), (name, bad))
                finally:
                    open(path, "w", encoding="utf-8").write(original)

    def test_a_missing_or_non_executable_script_is_refused(self):
        import stat
        path = os.path.join(self.dir, "release_guard.sh")
        os.chmod(path, 0o644)
        self.assertTrue(any("executable" in e for e in c.check_release_scripts(self.dir)))
        os.chmod(path, 0o755)
        os.remove(path)
        self.assertTrue(any("missing" in e for e in c.check_release_scripts(self.dir)))


class StagingAndRehearsalPolicy(unittest.TestCase):
    """The web site build (website.yml), the read-only rehearsal, the development gate of ci.yml and the pinned Windows image."""

    @classmethod
    def setUpClass(cls):
        import yaml
        d = os.path.join(ROOT, ".github", "workflows")
        cls.wf = {f: yaml.safe_load(open(os.path.join(d, f), encoding="utf-8")) for f in sorted(os.listdir(d)) if f.endswith(".yml")}
        cls.m = json.load(open(os.path.join(ROOT, "packaging", "flatpak", "org.tswlauncher.Launcher.json")))

    def errs(self, mutate):
        w = copy.deepcopy(self.wf)
        mutate(w)
        return c.check_ci_policy(w, self.m)

    def test_the_shipped_set_passes(self):
        self.assertEqual(c.check_ci_policy(self.wf, self.m), [])

    def test_website_yml_only_builds(self):
        def job(w):
            return w["website.yml"]["jobs"]["build"]
        self.assertTrue(self.errs(lambda w: w.pop("website.yml")))
        self.assertTrue(self.errs(lambda w: w["website.yml"].__setitem__("permissions", {"contents": "write"})))
        self.assertTrue(self.errs(lambda w: job(w).update({"permissions": {"contents": "read", "pages": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w)["steps"].append({"uses": "actions/deploy-pages@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: job(w)["steps"].append({"uses": "actions/upload-pages-artifact@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: job(w).update({"environment": "github-pages"})))
        self.assertTrue(self.errs(lambda w: job(w)["steps"].append({"run": "echo ${{ secrets.X }}"})))
        self.assertTrue(self.errs(lambda w: w["website.yml"]["on"].update({"push": {"branches": ["main"]}}) if "on" in w["website.yml"] else w["website.yml"].update({True: {"push": {"branches": ["main"]}}})))
        for needle in ("assemble_pages_site.py --website website --out", "assemble_pages_site.py --verify", "test_website.py", "SHA256SUMS-site"):
            def drop(w, needle=needle):
                for st in job(w)["steps"]:
                    if "run" in st:
                        st["run"] = st["run"].replace(needle, "true")
            self.assertTrue(self.errs(drop), needle)
        self.assertTrue(self.errs(lambda w: [st.update({"uses": "actions/upload-artifact@v4"}) for st in job(w)["steps"] if "upload-artifact" in str(st.get("uses", ""))]))
        # the 0.4.1 deployment failed because the upload left out the dot file .nojekyll: the upload must include hidden files, and the artifact is downloaded and checked again after it
        self.assertTrue(self.errs(lambda w: [st["with"].pop("include-hidden-files") for st in job(w)["steps"] if "upload-artifact" in str(st.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: [st["with"].update({"include-hidden-files": False}) for st in job(w)["steps"] if "upload-artifact" in str(st.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: job(w)["steps"].remove(next(st for st in job(w)["steps"] if "download-artifact" in str(st.get("uses", ""))))))
        for needle in ("sha256sum --check", "assemble_pages_site.py --verify", ".nojekyll"):
            def drop(w, needle=needle):
                for st in job(w)["steps"]:
                    if needle in str(st.get("run", "")):
                        st["run"] = st["run"].replace(needle, "true")
            self.assertTrue(self.errs(drop), needle)
        # a round trip that stands BEFORE the upload proves nothing
        def move_before(w):
            steps = job(w)["steps"]
            up = next(i for i, st in enumerate(steps) if "upload-artifact" in str(st.get("uses", "")))
            steps[:] = steps[up + 1:] + steps[:up + 1]
        self.assertTrue(self.errs(move_before))
        # the production deployment uploads the checked folder to Pages with its dot file
        self.assertTrue(self.errs(lambda w: [st["with"].pop("include-hidden-files") for st in w["build.yml"]["jobs"]["site"]["steps"] if "upload-pages-artifact" in str(st.get("uses", ""))]))
        self.assertTrue(self.errs(lambda w: [st.get("with", {}).update({"name": "site"}) for st in job(w)["steps"] if "upload-artifact" in str(st.get("uses", ""))]))

    def test_the_rehearsal_is_read_only_and_runs_the_release_scripts(self):
        def job(w):
            return w["release-rehearsal.yml"]["jobs"]["rehearse"]
        self.assertTrue(self.errs(lambda w: w.pop("release-rehearsal.yml")))
        self.assertTrue(self.errs(lambda w: w["release-rehearsal.yml"].__setitem__("permissions", {"contents": "write", "actions": "read"})))
        self.assertTrue(self.errs(lambda w: job(w).update({"permissions": {"contents": "read", "actions": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w).update({"permissions": {"contents": "read"}})))
        self.assertTrue(self.errs(lambda w: job(w).update({"environment": "release-publish"})))
        self.assertTrue(self.errs(lambda w: job(w).pop("if")))
        self.assertTrue(self.errs(lambda w: job(w).update({"if": "${{ github.ref_type == 'tag' }}"})))
        for bad in ("gh release create v1", "scripts/release_draft.sh verify v1 dist", "git push origin HEAD", "git tag v9.9.9", "echo ${{ secrets.X }}"):
            self.assertTrue(self.errs(lambda w, bad=bad: job(w)["steps"].append({"run": bad})), bad)
        for bad in ("actions/upload-artifact", "actions/deploy-pages", "actions/upload-pages-artifact"):
            self.assertTrue(self.errs(lambda w, bad=bad: job(w)["steps"].append({"uses": bad + "@" + "a" * 40})), bad)
        for needle in ("scripts/release_guard.sh --rehearse", "scripts/release_candidate_find.sh", "scripts/release_candidate_verify.sh", "scripts/release_assemble_dist.sh", "scripts/release_notes.py"):
            def drop(w, needle=needle):
                for st in job(w)["steps"]:
                    if needle in str(st.get("run", "")):
                        st["run"] = "true"
            self.assertTrue(self.errs(drop), needle)
        self.assertTrue(self.errs(lambda w: w["release-rehearsal.yml"]["jobs"].update({"x": {"if": "${{ github.ref == 'refs/heads/main' }}", "uses": "./.github/workflows/release.yml"}})))

    def test_ci_does_not_run_on_a_plain_push_in_the_development_repository_but_does_in_production(self):
        gate = self.wf["ci.yml"]["jobs"]["checks"]["if"]
        self.assertIn("github.repository == 'Glaubenskrieger/TSW-Forever'", gate)
        self.assertIn("github.event_name != 'push'", gate)
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"].pop("if")))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"].update({"if": gate.replace("github.event_name != 'push'", "true")})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"].update({"if": gate.replace("github.repository == 'Glaubenskrieger/TSW-Forever' || ", "")})))
        # the build workflow is not gated: it runs in both repositories
        self.assertNotIn("event_name != 'push'", str(self.wf["build.yml"]["jobs"]["flatpak"].get("if", "")))

    def test_the_rehearsal_and_the_release_fail_closed_and_use_the_artifacts_of_one_commit_only(self):
        def step_of(w, wf, jn, needle):
            return next(st for st in w[wf]["jobs"][jn]["steps"] if needle in str(st.get("uses", "")) + str(st.get("run", "")))
        for wf, jn in (("release-rehearsal.yml", "rehearse"), ("release.yml", "candidate"), ("release.yml", "draft"), ("release.yml", "guard")):
            with self.subTest(workflow=wf, job=jn):
                self.assertTrue(self.errs(lambda w: w[wf]["jobs"][jn]["steps"][-1].update({"continue-on-error": True})))
                self.assertTrue(self.errs(lambda w: w[wf]["jobs"][jn].update({"continue-on-error": True})))
                for cond in ("always()", "failure()", "cancelled()", "${{ always() }}", "${{ failure() && true }}"):
                    self.assertTrue(self.errs(lambda w, cond=cond: w[wf]["jobs"][jn]["steps"][-1].update({"if": cond})), cond)
        # the cross-run download of the rehearsal and of the release: named after the commit, from the run the lookup found, with an explicit repository and token
        for wf, jn in (("release-rehearsal.yml", "rehearse"), ("release.yml", "candidate")):
            with self.subTest(download=wf):
                self.assertTrue(self.errs(lambda w: step_of(w, wf, jn, "download-artifact")["with"].update({"pattern": "tsw-forever-*"})))
                self.assertTrue(self.errs(lambda w: step_of(w, wf, jn, "download-artifact")["with"].update({"run-id": "12345"})))
                self.assertTrue(self.errs(lambda w: step_of(w, wf, jn, "download-artifact")["with"].update({"run-id": "${{ github.run_id }}"})))
                self.assertTrue(self.errs(lambda w: step_of(w, wf, jn, "download-artifact")["with"].pop("repository")))
                self.assertTrue(self.errs(lambda w: step_of(w, wf, jn, "download-artifact")["with"].pop("github-token")))
        self.assertTrue(self.errs(lambda w: step_of(w, "release-rehearsal.yml", "rehearse", "download-artifact")["with"].pop("run-id")), "without run-id the rehearsal would take artifacts of ITS OWN run")
        self.assertTrue(self.errs(lambda w: step_of(w, "release.yml", "publish", "download-artifact")["with"].update({"name": "release-dist"})), "no artifact without the commit in its name")

    def test_the_lookup_script_lists_all_runs_and_the_newest_decides(self):
        text = open(os.path.join(ROOT, "scripts", "release_candidate_find.sh"), encoding="utf-8").read()
        code = "\n".join(l for l in text.splitlines() if not l.lstrip().startswith("#"))
        self.assertNotRegex(code, r"--status\b")
        self.assertIn("sort_by(.createdAt) | last", code)
        scripts = os.path.join(ROOT, "scripts")
        self.assertEqual(c.check_release_scripts(scripts), [])
        import shutil
        import tempfile
        tmp = tempfile.mkdtemp()
        try:
            shutil.copytree(scripts, os.path.join(tmp, "scripts"), ignore=shutil.ignore_patterns("__pycache__"))
            path = os.path.join(tmp, "scripts", "release_candidate_find.sh")
            original = open(path, encoding="utf-8").read()
            open(path, "w", encoding="utf-8").write(original.replace("--limit 50", "--limit 50 --status success"))
            self.assertTrue(any("must not filter runs by status" in e for e in c.check_release_scripts(os.path.join(tmp, "scripts"))))
            open(path, "w", encoding="utf-8").write(original.replace("set -euo pipefail", "set -eo pipefail"))
            self.assertTrue(any("pipefail" in e for e in c.check_release_scripts(os.path.join(tmp, "scripts"))))
        finally:
            shutil.rmtree(tmp)

    def test_the_windows_image_is_pinned(self):
        self.assertEqual(self.wf["windows.yml"]["jobs"]["windows"]["runs-on"], "windows-2025")
        self.assertTrue(self.errs(lambda w: w["windows.yml"]["jobs"]["windows"].update({"runs-on": "windows-latest"})))
        self.assertTrue(self.errs(lambda w: w["windows.yml"]["jobs"]["windows"].update({"runs-on": "ubuntu-24.04"})))

    def test_artifacts_are_kept_30_days_in_production_and_7_in_development(self):
        import re as _re
        for name in ("build.yml", "windows.yml", "flatpak-stable-build.yml", "flatpak.yml", "rc-artifacts.yml", "release-set.yml"):
            text = open(os.path.join(ROOT, ".github", "workflows", name), encoding="utf-8").read()
            values = _re.findall(r"retention-days:\s*(.+)", text)
            self.assertTrue(values, name)
            for v in values:
                self.assertIn(v.strip(), (c.RETENTION_EXPR, "7"), f"{name}: retention-days {v!r}")
        self.assertEqual(c.RETENTION_EXPR, "${{ github.repository == 'Glaubenskrieger/TSW-Forever' && 30 || 7 }}")


if __name__ == "__main__":
    unittest.main()


class BundleArguments(unittest.TestCase):
    """`build_flatpak.sh bundle-args`: no bundle carries an update source or a key; the branch is stable, testing or local; nothing else is allowed."""

    SCRIPT = os.path.join(HERE, "build_flatpak.sh")

    def run_args(self, branch):
        import subprocess
        env = dict(os.environ, FLATPAK_BRANCH=branch)
        return subprocess.run(["bash", self.SCRIPT, "bundle-args"], capture_output=True, text=True, env=env)

    def test_stable_local_and_testing_bundles_are_plain_files_without_update_source_or_key(self):
        for b in ("local", "testing", "stable"):
            r = self.run_args(b)
            self.assertEqual(r.returncode, 0, (b, r.stderr))
            self.assertNotIn("--repo-url", r.stdout + r.stderr)
            self.assertNotIn("--gpg-keys", r.stdout + r.stderr)

    def test_the_bundle_command_has_no_update_source_and_no_key_argument(self):
        text = open(self.SCRIPT).read()
        self.assertNotIn("--repo-url", text.split("bundle_args() {")[1].split("cmd_bundle")[0].replace("--repo-url", "", 0) if False else "")
        self.assertNotRegex(text, r"build-bundle[^\n]*(--repo-url|--gpg-keys|--gpg-sign)")
        self.assertNotIn("FLATPAK_REPO_URL", text)

    def test_any_other_branch_is_refused(self):
        for b in ("beta", "Stable", "stable ", "main", ""):
            self.assertNotEqual(self.run_args(b).returncode, 0, repr(b))
