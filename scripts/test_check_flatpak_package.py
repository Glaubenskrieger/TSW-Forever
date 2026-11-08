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


RELEASE = os.path.join(HERE, "..", ".github", "workflows", "release-flatpak.yml")


class ReleaseWorkflowPolicy(unittest.TestCase):
    def setUp(self):
        self.text = open(RELEASE).read()
        self.m = json.load(open(MANIFEST))

    def test_the_shipped_release_workflow_passes(self):
        self.assertEqual(c.check_release_workflow(self.text, self.m), [])

    def test_automatic_triggers_missing_confirmation_or_environment_and_foreign_secrets_are_rejected(self):
        self.assertTrue(c.check_release_workflow(self.text.replace("workflow_dispatch:", "push:\n    branches: [main]\n  workflow_dispatch:", 1), self.m))
        self.assertTrue(c.check_release_workflow(self.text.replace("inputs.confirm == 'PUBLISH'", "true"), self.m))
        self.assertTrue(c.check_release_workflow(self.text.replace("environment: flatpak-release", "", 1), self.m))
        self.assertTrue(c.check_release_workflow(self.text + "\n      - env: { T: ${{ secrets.OTHER_TOKEN }} }\n", self.m))
        self.assertTrue(c.check_release_workflow(self.text.replace("scripts/pin_actions.sh --check", "true"), self.m))
        self.assertTrue(c.check_release_workflow(self.text.replace("contents: read", "contents: write", 1), self.m))
        self.assertTrue(c.check_release_workflow(self.text + "\n      - run: gh release create v1\n", self.m))

    def test_the_build_workflow_still_has_no_secrets_and_cannot_publish(self):
        build = open(WORKFLOW).read()
        self.assertNotIn("secrets.", build)
        self.assertNotIn("pages: write", build)


WORKFLOWS = os.path.join(HERE, "..", ".github", "workflows")


def on(w, name):
    """The `on:` block (PyYAML reads the key `on` as the boolean True)."""
    d = w[name]
    return d["on"] if "on" in d else d[True]


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

    def test_the_tag_release_is_owner_only_read_only_except_publish_and_publishes_four_files_through_a_draft(self):
        def job(w, n):
            return w["release.yml"]["jobs"][n]

        def drop_step(n, needle):
            def f(w):
                st = job(w, n)["steps"]
                st[:] = [x for x in st if needle not in str(x.get("run", ""))]
            return f

        self.assertTrue(self.errs(lambda w: w.pop("release.yml")))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml").update({"workflow_dispatch": None})))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml")["push"].update({"branches": ["main"]})))
        self.assertTrue(self.errs(lambda w: on(w, "release.yml")["push"].update({"tags": ["*"]})))
        self.assertTrue(self.errs(lambda w: w["release.yml"].update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "set").update({"permissions": {"contents": "write"}})))
        self.assertTrue(self.errs(lambda w: job(w, "set").update({"uses": "./.github/workflows/rc-artifacts.yml"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish").update({"permissions": {"contents": "read"}})))
        self.assertTrue(self.errs(lambda w: job(w, "publish").update({"needs": ["guard"]})))
        for n in ("guard", "set", "publish"):
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).pop("if")), n)
            self.assertTrue(self.errs(lambda w, n=n: job(w, n).update({"if": "${{ github.ref_type == 'tag' }}"})), n)
        for needle in ("merge-base --is-ancestor", "gh release view", "gh release download", "check_release_set.py", "check_public_source.py", "sha256sum --check"):
            self.assertTrue(self.errs(drop_step("publish", needle)), needle)
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "gh release upload v1 x --clobber"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "git tag v1 && git push origin v1"})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "x", "env": {"T": "${{ secrets.T }}"}})))
        self.assertTrue(self.errs(lambda w: job(w, "publish")["steps"].append({"run": "echo ${{ github.ref_name }}"})))
        # only release.yml may react to a tag: every other workflow still must not
        self.assertTrue(self.errs(lambda w: on(w, "release-set.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "windows.yml").update({"push": {"tags": ["v*"]}})))
        self.assertTrue(self.errs(lambda w: on(w, "release-set.yml").update({"workflow_dispatch": {"inputs": {"ref": {"type": "string"}}}})))

    def test_a_package_build_on_pull_requests_or_other_branches_is_rejected(self):
        self.assertTrue(self.errs(lambda w: on(w, "flatpak.yml").update({"pull_request": None})))
        self.assertTrue(self.errs(lambda w: on(w, "flatpak.yml")["push"].update({"branches": ["main", "dev"]})))

    def test_a_missing_job_guard_or_main_ancestry_check_is_rejected(self):
        self.assertTrue(self.errs(lambda w: w["flatpak.yml"]["jobs"]["flatpak"].pop("if")))
        self.assertTrue(self.errs(lambda w: w["release-flatpak.yml"]["jobs"]["deploy"].pop("if")))
        def drop(w):
            st = w["flatpak.yml"]["jobs"]["flatpak"]["steps"]
            st[:] = [x for x in st if "merge-base" not in str(x.get("run", ""))]
        self.assertTrue(self.errs(drop))

    def test_the_check_workflow_builds_and_publishes_nothing(self):
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"run": "scripts/build_flatpak.sh build"})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"uses": "actions/upload-artifact@" + "a" * 40})))
        self.assertTrue(self.errs(lambda w: w["ci.yml"]["jobs"]["checks"]["steps"].append({"run": "x", "env": {"T": "${{ secrets.T }}"}})))

    def test_the_release_is_manual_with_only_confirm_and_channel(self):
        def free_ref(w):
            on(w, "release-flatpak.yml")["workflow_dispatch"]["inputs"]["ref"] = {"type": "string"}
        self.assertTrue(self.errs(free_ref))
        def free_channel(w):
            on(w, "release-flatpak.yml")["workflow_dispatch"]["inputs"]["channel"] = {"type": "string"}
        self.assertTrue(self.errs(free_channel))
        self.assertTrue(self.errs(lambda w: on(w, "release-flatpak.yml").update({"push": {"branches": ["main"]}})))
        self.assertTrue(self.errs(lambda w: w["release-flatpak.yml"]["jobs"]["build"]["steps"].append({"run": "echo ${{ inputs.channel }}"})))

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
        self.assertTrue(self.text_errs(lambda t: t.replace("actions/upload-artifact@ea165f8d65b6e75b540449e92b4886f43607fa02", "actions/upload-artifact@v4", 1)))
        self.assertTrue(self.text_errs(lambda t: t.replace("retention-days: 30", "retention-days: 90", 1)))
        swapped = self.text.replace("scripts/build_flatpak.sh test", "@@T", 1).replace("run: scripts/build_flatpak.sh build", "run: scripts/build_flatpak.sh test", 1).replace("@@T", "scripts/build_flatpak.sh build", 1)
        self.assertTrue(c.check_rc_workflow(swapped, self.m))
        self.assertTrue(self.text_errs(lambda t: t.replace('FREEDESKTOP_VERSION: "25.08"', 'FREEDESKTOP_VERSION: "24.08"', 1)))

    def test_the_main_and_flatpak_policies_still_apply_to_the_other_workflows(self):
        self.assertTrue(self.doc_errs(lambda w: on(w, "flatpak.yml")["push"].update({"tags": ["v*"]})))
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
        self.checks = {"flatpak.yml": c.check_workflow, "rc-artifacts.yml": c.check_rc_workflow, "release-flatpak.yml": c.check_release_workflow}
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


if __name__ == "__main__":
    unittest.main()
