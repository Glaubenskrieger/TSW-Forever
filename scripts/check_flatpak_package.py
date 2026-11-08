#!/usr/bin/env python3
"""Policy checks for the Flatpak package, run before any build (no network, no Flatpak needed).

  check_flatpak_package.py manifest <manifest.json>   sandbox permissions, sources and build commands
  check_flatpak_package.py tree <stage-dir>           the staged source tree contains no game data and nothing private
  check_flatpak_package.py workflow <workflow.yml> <manifest.json>   the CI definition follows the project rules
  check_flatpak_package.py release-workflow <workflow.yml> <manifest.json>   the publishing workflow is manual, confirmed, gated and pinned-checked
  check_flatpak_package.py rc-workflow <workflow.yml> <manifest.json>   the artifact workflow builds the Flatpak bundle and the source archive and cannot publish anything
  check_flatpak_package.py ci-policy <workflows-dir> <manifest.json>   the main-only build policy across ALL workflows (needs PyYAML, present on the runners after `apt install python3-yaml`)

Exit code 0 = fine, 1 = a rule is violated (each violation is printed). Usage as a module: see test_check_flatpak_package.py.
"""
import json
import os
import re
import sys

# Sandbox permissions that are never acceptable for this launcher (each one would be a sandbox escape or a broad grant).
FORBIDDEN_FINISH_ARGS = [
    r"^--filesystem=(host|host-os|host-etc|home|/|~|xdg-config|xdg-data|xdg-cache)(:.*)?$",
    r"^--device=all$",
    r"^--socket=(session-bus|system-bus|ssh-auth|gpg-agent|fallback-x11)$",
    r"^--talk-name=org\.freedesktop\.Flatpak.*$",
    r"^--system-talk-name=.*$",
    r"^--own-name=.*$",
    r"^--allow=(devel|multiarch-nonsense)$",
    r"^--env=(LD_PRELOAD|LD_LIBRARY_PATH).*$",
]
# The granted filesystem paths we accept (least privilege): exactly ONE folder - the installation folder (Wine prefix with the game inside, runtime, downloads,
# caches, logs; decision of the owner, 2026-11-01). Only the small configuration lives in the app's own data folder; a host file is chosen through the file-chooser portal.
# Every other grant (home, ~/Games as a whole, Steam folders...) is rejected; a new one needs a change here AND a documented reason.
ALLOWED_FILESYSTEM = {"~/Games/The Secret World:create"}

FORBIDDEN_SUFFIXES = (".exe", ".dll", ".rdb", ".bxml", ".swf", ".pak", ".dmp", ".log", ".msi", ".cab", ".sys", ".so", ".dylib")
FORBIDDEN_DIRS = {"analysis", "reports", "research", "native", "logs", "tasks", "patches", "config", "t", ".agents", ".claude", ".codex", ".aws", ".git", "target"}
FORBIDDEN_NAMES = {"Prefs_3.xml", "Prefs_3.bxml", "ClientLog.txt", "last-launch.txt", "config", "detected", "install-state"}
SIZE_CAP_BYTES = 40 * 1024 * 1024  # without vendor/: sources, icons, translations


def check_manifest(m):
    errs = []
    if m.get("app-id") != "org.tswlauncher.Launcher":
        errs.append("app-id must be org.tswlauncher.Launcher (it must match the desktop entry and the icons)")
    if m.get("runtime") != "org.freedesktop.Platform" or m.get("sdk") != "org.freedesktop.Sdk":
        errs.append("runtime/sdk must be the Freedesktop platform and SDK")
    if not re.fullmatch(r"\d\d\.\d\d", str(m.get("runtime-version", ""))):
        errs.append("runtime-version must be pinned (YY.MM)")
    for arg in m.get("finish-args", []):
        for pat in FORBIDDEN_FINISH_ARGS:
            if re.match(pat, arg):
                errs.append(f"forbidden finish-arg {arg!r}")
        if arg.startswith("--filesystem="):
            path = arg[len("--filesystem="):]
            if path not in ALLOWED_FILESYSTEM:
                errs.append(f"filesystem permission {path!r} is not on the allow-list {sorted(ALLOWED_FILESYSTEM)}")
    # --allow=multiarch: the 32-bit client (and Wine's 32-bit parts) die with SIGSYS under Flatpak's default seccomp filter without it (measured 2026-10-31: `wineboot` -> SIGSYS)
    for want in ("--socket=x11", "--device=dri", "--allow=multiarch"):
        if want not in m.get("finish-args", []):
            errs.append(f"missing finish-arg {want}")
    for name in ("org.freedesktop.Platform.Compat.i386", "org.freedesktop.Platform.GL32"):
        if name not in m.get("add-extensions", {}):
            errs.append(f"the 32-bit client needs the extension {name}")
    if "org.freedesktop.Sdk.Extension.rust-stable" not in m.get("sdk-extensions", []):
        errs.append("sdk-extensions must contain rust-stable")
    for mod in m.get("modules", []):
        for src in mod.get("sources", []):
            t = src.get("type")
            if t == "dir":
                if ".." in src.get("path", "").split("/")[:1] and src["path"] != "../../build/flatpak/src":
                    errs.append(f"dir source {src['path']!r} must be the staged tree ../../build/flatpak/src")
            elif t == "archive":
                if not re.fullmatch(r"[0-9a-f]{64}", src.get("sha256", "")):
                    errs.append("archive source without a pinned sha256")
            else:
                errs.append(f"source type {t!r} is not allowed (no git, no downloads of unpinned files, no shell sources)")
            for k in ("url", "mirror-urls"):
                if k in src and t != "archive":
                    errs.append(f"{k} is only allowed on a pinned archive")
        for cmd in mod.get("build-commands", []):
            if re.search(r"\b(curl|wget|git clone|pip install|npm install|sudo)\b", cmd):
                errs.append(f"build command downloads or elevates: {cmd[:60]!r}")
            if "--offline" not in cmd and "cargo" in cmd:
                errs.append(f"cargo must run --offline: {cmd[:60]!r}")
    return errs


def check_tree(root):
    errs, total = [], 0
    for dirpath, dirnames, filenames in os.walk(root):
        rel = os.path.relpath(dirpath, root)
        if rel == "vendor" or rel.startswith("vendor" + os.sep):
            dirnames[:] = []  # third-party crates: licensed code, checked by their lockfile, not by this scan
            continue
        for d in list(dirnames):
            if d in FORBIDDEN_DIRS:
                errs.append(f"forbidden directory in the package: {os.path.join(rel, d)}")
                dirnames.remove(d)
        for f in filenames:
            p = os.path.join(dirpath, f)
            r = os.path.relpath(p, root)
            if f.lower().endswith(FORBIDDEN_SUFFIXES) or f in FORBIDDEN_NAMES:
                errs.append(f"forbidden file in the package: {r}")
            if os.path.islink(p):
                errs.append(f"symbolic link in the package: {r}")
                continue
            total += os.path.getsize(p)
    if total > SIZE_CAP_BYTES:
        errs.append(f"the staged sources are {total} bytes (cap {SIZE_CAP_BYTES}): something large was copied by accident")
    return errs


# Packages that `flatpak-builder` needs on the HOST but that Debian/Ubuntu only RECOMMEND, so `apt-get install --no-install-recommends` leaves them out:
#   elfutils        - flatpak-builder runs `eu-strip` and `eu-elfcompress` to split and compress the debug symbols (recommended by flatpak-builder);
#   librsvg2-common - the SVG loader of gdk-pixbuf (recommended by librsvg2-2): without it `appstreamcli compose` (AppStream 1.0.2 of Ubuntu 24.04) cannot read the
#                     scalable icon and fails with `file-read-error` / `filters-but-no-output` (the known failure of --no-install-recommends on 24.04 runners).
BUILDER_HOST_PACKAGES = ("elfutils", "librsvg2-common")


def _builder_host_tools(text, label):
    """A workflow that builds the Flatpak must install the packages of BUILDER_HOST_PACKAGES in the same `apt-get install` line as flatpak-builder."""
    lines = re.findall(r"apt-get\s+install[^\n]*", text)
    builds = re.search(r"build_flatpak\.sh\s+(build|bundle|publish-repo|all)\b", text) or any("flatpak-builder" in l for l in lines)
    if not builds:
        return []
    install = next((l for l in lines if "flatpak-builder" in l), "")
    missing = [p for p in BUILDER_HOST_PACKAGES if not re.search(r"(?<![\w-])" + re.escape(p) + r"(?![\w-])", install)]
    return [f"{label}: the runner must install {', '.join(missing)} together with flatpak-builder (host tools recommended but not required by the package)"] if missing else []


def check_workflow(text, manifest):
    """Text-level rules for the GitHub Actions file (no YAML library needed)."""
    errs = []
    for pat, why in [
        (r"\bgit\s+(push|commit|tag)\b", "CI must not commit, push or tag"),
        (r"\bgh\s+release\b|softprops/action-gh-release|actions/create-release", "CI must not publish releases"),
        (r"\bsecrets\.", "no secrets"),
        (r"\bsudo\b(?!\s+(apt-get\s+(update|install)|sysctl\s+-w\s+kernel\.apparmor_restrict_unprivileged_userns=0\b))", "sudo only for apt-get and the AppArmor user-namespace setting on the CI runner"),
        (r"\.exe\b|\bmsi\b|\bappimage\b|\.deb\b|\.rpm\b|\bsnap\b", "only the Flatpak is built, no other package format"),
        (r"permissions:\s*\n\s+(contents|packages|id-token):\s*write", "write permissions are not allowed"),
        (r"pull_request_target", "pull_request_target runs untrusted code with privileges"),
    ]:
        if re.search(pat, text, re.I):
            errs.append(f"workflow: {why} ({pat})")
    if not re.search(r"permissions:\s*\n\s+contents:\s*read", text):
        errs.append("workflow: top-level permissions must be contents: read")
    if "persist-credentials: false" not in text:
        errs.append("workflow: checkout must not persist credentials")
    for step in ("scripts/build_flatpak.sh test", "scripts/build_flatpak.sh build"):
        if step not in text:
            errs.append(f"workflow: missing step {step!r}")
    if text.find("build_flatpak.sh test") > text.find("build_flatpak.sh build") > -1:
        errs.append("workflow: tests must run before the package is built")
    for uses in re.findall(r"uses:\s*(\S+)", text):
        if not uses.startswith("./") and not re.fullmatch(r".+@[0-9a-f]{40}", uses):
            errs.append(f"workflow: action {uses!r} is not pinned to a 40-character commit SHA")
    m = re.search(r'FREEDESKTOP_VERSION:\s*"([^"]+)"', text)
    if not m or m.group(1) != str(manifest.get("runtime-version")):
        errs.append("workflow: FREEDESKTOP_VERSION must equal the manifest's runtime-version")
    errs += _builder_host_tools(text, "workflow")
    return errs


def check_release_workflow(text, manifest):
    errs = []
    on_block = text.split("permissions:", 1)[0]
    if re.search(r"^\s+(push|pull_request|pull_request_target|schedule|workflow_run|release):", on_block, re.M):
        errs.append("release workflow: it may only be started manually (workflow_dispatch)")
    if "workflow_dispatch:" not in text:
        errs.append("release workflow: missing workflow_dispatch")
    if "inputs.confirm == 'PUBLISH'" not in text:
        errs.append("release workflow: the job must require the typed confirmation PUBLISH")
    if text.count("environment: flatpak-release") < 2:
        errs.append("release workflow: both jobs must use the protected environment flatpak-release")
    if "scripts/pin_actions.sh --check" not in text:
        errs.append("release workflow: must refuse unpinned actions")
    for pat, why in [(r"\bgit\s+(push|commit|tag)\b", "no git writes"), (r"\bgh\s+release\b|action-gh-release|create-release", "no release creation"),
                     (r"pull_request_target", "pull_request_target"), (r"persist-credentials:\s*true", "credentials must not persist"),
                     (r"\.exe\b|\.msi\b|appimage|\.deb\b|\.rpm\b", "only the Flatpak is published")]:
        if re.search(pat, text, re.I):
            errs.append(f"release workflow: {why}")
    for name in re.findall(r"secrets\.([A-Za-z0-9_]+)", text):
        if name not in ("FLATPAK_GPG_PRIVATE_KEY", "FLATPAK_GPG_KEY_ID", "FLATPAK_GPG_PASSPHRASE"):
            errs.append(f"release workflow: unexpected secret {name}")
    perms = re.findall(r"^\s+([a-z-]+):\s*(read|write)\s*$", text, re.M)
    for k, v in perms:
        if v == "write" and k not in ("pages", "id-token"):
            errs.append(f"release workflow: write permission {k} is not allowed")
    if "persist-credentials: false" not in text:
        errs.append("release workflow: checkout must not persist credentials")
    m = re.search(r'FREEDESKTOP_VERSION:\s*"([^"]+)"', text)
    if not m or m.group(1) != str(manifest.get("runtime-version")):
        errs.append("release workflow: FREEDESKTOP_VERSION must equal the manifest's runtime-version")
    if text.find("build_flatpak.sh test") == -1 or text.find("build_flatpak.sh test") > text.find("publish-repo"):
        errs.append("release workflow: tests must run before the repository is built")
    errs += _builder_host_tools(text, "release workflow")
    return errs


BUILD_MARKERS = re.compile(r"flatpak-builder|build_flatpak\.sh\s+(build|bundle|publish-repo|all|vendor)\b|flatpak\s+build-|actions/upload-artifact|upload-pages-artifact|deploy-pages|ostree\s+(commit|pull)")
MAIN_GUARD = "github.ref == 'refs/heads/main'"


# The disk of a GitHub runner is small: the rc-artifacts workflow measures it at fixed points and removes only temporary build data that no later step needs.
RC_MEASURING_POINTS = ("1 runner start", "2 after installing runtime, SDK and Rust", "3 after the tests, before cleanup 1", "4 after cleanup 1",
                       "5 after the Flatpak build and bundle, before cleanup 2", "6 after cleanup 2", "7 after the source archive check, before cleanup 3",
                       "8 after cleanup 3", "9 end of the job")
RC_REMOVABLE = {"launcher/target", "launcher-gui/target", "patcher/target", "build/flatpak/state", "build/flatpak/build-dir", "build/flatpak/repo", "build/source-check"}
RC_UNINSTALLABLE = ("org.freedesktop.Sdk.Extension.rust-stable//", "org.freedesktop.Sdk//")

# The official Linux artifacts are the Flatpak bundle and the source archive. Nothing distribution-specific is built or published (decision of 2026-11-02): no
# Debian/Ubuntu package, no Arch package or PKGBUILD, no RPM, no AppImage, and therefore no container image for such builds.
RC_ARTIFACT_NAMES = ("tsw-forever-${version}.flatpak", "tsw-forever-${version}-src.tar.gz", "SHA256SUMS")
RC_FORBIDDEN_FORMATS = (r"\bdocker\b|\.deb\b|\.rpm\b|pkg\.tar|PKGBUILD|makepkg|\bdpkg|\bpacman\b|\barchlinux\b|appimage|build_deb|ci_build|DEBIAN_BUILD|ARCH_BUILD|\bfpm\b|alien\b")


def _code_only(text):
    """The workflow without its comment lines (the header explains what is NOT built and may name the formats)."""
    return "\n".join(l for l in text.splitlines() if not l.strip().startswith("#"))


def _rc_artifact_rules(text):
    """The release set of rc-artifacts consists of the Flatpak bundle, the source archive and SHA256SUMS - nothing else is copied, listed or uploaded."""
    errs = []
    code = _code_only(text)
    if re.search(RC_FORBIDDEN_FORMATS, code, re.I):
        errs.append("rc workflow: only the Flatpak bundle and the source archive are built (no .deb, Arch package, PKGBUILD, RPM, AppImage and no container build)")
    copies = [c for c in re.findall(r'cp\s+[^\n]*?"\$set/([^"]*)"', code) if c]   # named targets; the source archive is copied into the folder (checked below)
    if sorted(copies) != sorted(("tsw-forever-${version}.flatpak",)):
        errs.append(f"rc workflow: exactly the Flatpak bundle is copied by name into the release set, found {copies}")
    if not re.search(r'cp\s+"build/native/tsw-forever-\$\{version\}-src\.tar\.gz"\s+"\$set/"', code):
        errs.append("rc workflow: the source archive must be copied into the release set")
    sums = re.findall(r"sha256sum\s+([^>\n]*)>\s*SHA256SUMS", code)
    if len(sums) != 1 or sorted(sums[0].split()) != sorted(("tsw-forever-*.flatpak", "tsw-forever-*-src.tar.gz")):
        errs.append(f"rc workflow: SHA256SUMS lists exactly the Flatpak bundle and the source archive, found {sums}")
    paths = re.findall(r"^\s*path:\s*(\S+)", code, re.M)
    if paths != ["build/rc/"]:
        errs.append(f"rc workflow: only the release set folder build/rc/ is uploaded, found {paths}")
    return errs


def _rc_disk_rules(text):
    errs = []
    pos = []
    for label in RC_MEASURING_POINTS:
        at = text.find('scripts/ci_disk_report.sh "' + label)
        if at < 0:
            errs.append(f"rc workflow: missing disk measuring point {label!r}")
        pos.append(at)
    if all(p >= 0 for p in pos) and pos != sorted(pos):
        errs.append("rc workflow: the disk measuring points are not in the order of the build phases")
    removals = [(m.start(), m.group(1).split()) for m in re.finditer(r"^\s*(?:-\s+)?(?:run:\s*)?rm\s+-rf\s+([^\n]+)$", text, re.M)]
    for at, targets in removals:
        for t in targets:
            if t not in RC_REMOVABLE:
                errs.append(f"rc workflow: rm -rf of {t!r} is not allowed (only temporary build data: {sorted(RC_REMOVABLE)})")
    for line in text.splitlines():
        code = re.sub(r"^(-\s+)?(run:\s*)?", "", line.strip())   # a one-line `- run: <command>` is judged like the command
        if re.search(r"(^|[;&|(]\s*)(sudo\s+)?(rm|rmdir|unlink|shred|find\s[^\n]*-delete)\b", code) and not re.fullmatch(r"rm -rf [^;&|$`]+", code):
            errs.append("rc workflow: files are removed only with the fixed `rm -rf <build directory>` lines")
            break
    cleanups = ((0, "3 after the tests", "4 after cleanup 1"), (1, "5 after the Flatpak build", "6 after cleanup 2"), (2, "7 after the source archive check", "8 after cleanup 3"))
    for index, before, after in cleanups:
        if len(removals) <= index:
            errs.append(f"rc workflow: cleanup {index + 1} is missing")
            continue
        b, a_ = text.find('scripts/ci_disk_report.sh "' + before), text.find('scripts/ci_disk_report.sh "' + after)
        if not (0 <= b < removals[index][0] < a_):
            errs.append(f"rc workflow: cleanup {index + 1} must be measured before and after")
    if len(removals) >= 2 and not (text.find("scripts/build_flatpak.sh bundle") < removals[1][0] < text.find("scripts/native_stage.sh")):
        errs.append("rc workflow: the Flatpak build state may only be removed after the bundle is exported and before the source archive is staged")
    if len(removals) >= 3 and not (text.find("check --locked --offline") < removals[2][0] < text.find("scripts/check_release_set.py")):
        errs.append("rc workflow: the source archive check may only be removed after it ran and before the release set is assembled")
    for line in re.findall(r"flatpak\s+uninstall[^\n]*", text):
        refs = re.findall(r'"([^"]+)"', line)
        if not refs or not all(r.startswith(RC_UNINSTALLABLE) for r in refs):
            errs.append("rc workflow: only the SDK and its Rust extension may be uninstalled (the Platform runtime is needed for the installation check)")
    if "always()" not in text[text.rfind("- name: Disk report - end"):] :
        errs.append("rc workflow: the last measurement must run also after a failure (if: always())")
    return errs


def check_rc_workflow(text, manifest):
    """Text-level rules of `rc-artifacts.yml`, which builds the Flatpak bundle and the source archive and checks them: it may NOT publish anything (no release, no
    push/tag/commit, no secret, no write permission), may only upload workflow artifacts, and builds no distribution-specific package."""
    errs = []
    for pat, why in [
        (r"\bgit\s+(push|commit|tag)\b", "CI must not commit, push or tag"),
        (r"\bgh\s+(release|api)\b|softprops/action-gh-release|actions/create-release|upload-release-asset", "this workflow must not create or change a GitHub Release"),
        (r"\bsecrets\.", "no secrets"),
        (r"\bsudo\b(?!\s+(apt-get\s+(update|install)|sysctl\s+-w\s+kernel\.apparmor_restrict_unprivileged_userns=0\b))", "sudo only for apt-get and the AppArmor user-namespace setting on the CI runner"),
        (r"permissions:\s*\n\s+(contents|packages|id-token|pages|actions|deployments|releases):\s*write", "write permissions are not allowed"),
        (r"pull_request_target|workflow_run|\bschedule:", "only a manual start is allowed"),
        (r"deploy-pages|upload-pages-artifact|publish-repo", "nothing is published from this workflow"),
    ]:
        if re.search(pat, text, re.I):
            errs.append(f"rc workflow: {why} ({pat})")
    if not re.search(r"permissions:\s*\n\s+contents:\s*read", text):
        errs.append("rc workflow: top-level permissions must be contents: read")
    if "persist-credentials: false" not in text:
        errs.append("rc workflow: checkout must not persist credentials")
    for needed, why in [
        ("merge-base --is-ancestor", "the commit must be verified to be part of main"),
        ("scripts/version.py check", "the version must be checked"),
        ("GITHUB_REF_TYPE", "a tag must be compared with APP_VERSION"),
        ("scripts/pin_actions.sh --check", "unpinned actions must be refused"),
        ("scripts/build_flatpak.sh test", "tests must run"),
        ("scripts/build_flatpak.sh build", "the Flatpak must be built"),
        ("scripts/build_flatpak.sh bundle", "the Flatpak bundle must be exported"),
        ("rm -rf launcher/target launcher-gui/target patcher/target", "the test builds fill the runner disk: they must be removed before the Flatpak is built"),
        ("scripts/native_stage.sh", "the source archive must be staged"),
        ("sha256sum", "the integrity of the artifacts must be recorded"),
        ("scripts/check_release_set.py", "the release set must be checked for consistency"),
        ("scripts/check_release_artifacts.py", "the artifacts must be audited for Microsoft/Funcom files"),
        ("--command=tsw-launch", "the installed Flatpak must be started once"),
        ("actions/upload-artifact@", "the result is uploaded as a workflow artifact"),
    ]:
        if needed not in text:
            errs.append(f"rc workflow: missing {needed!r} ({why})")
    if '[ "$first" = "$second" ] ||' not in text:
        errs.append("rc workflow: the two source archive hashes must be compared and a mismatch must fail")
    if text.count("scripts/native_stage.sh") < 2:
        errs.append("rc workflow: the source archive must be built twice and compared (reproducibility)")
    if not re.search(r'cargo\s+"\+\$\{RUST_VERSION\}"\s+check\s+--locked\s+--offline', text):
        errs.append("rc workflow: the unpacked source archive must be built (cargo check --locked --offline with the pinned Rust version)")
    if text.find("scripts/build_flatpak.sh test") > text.find("scripts/build_flatpak.sh build") > -1:
        errs.append("rc workflow: tests must run before the Flatpak is built")
    cleanup = text.find("rm -rf launcher/target launcher-gui/target patcher/target")
    if cleanup > -1 and cleanup > text.find("scripts/build_flatpak.sh build"):
        errs.append("rc workflow: the test builds must be removed before the Flatpak is built")
    errs += _rc_artifact_rules(text)
    errs += _rc_disk_rules(text)
    for uses in re.findall(r"uses:\s*(\S+)", text):
        if not uses.startswith("./") and not re.fullmatch(r".+@[0-9a-f]{40}", uses):
            errs.append(f"rc workflow: action {uses!r} is not pinned to a 40-character commit SHA")
    for days in re.findall(r"retention-days:\s*(\d+)", text):
        if int(days) > 30:
            errs.append("rc workflow: artifacts are kept 30 days at most")
    m = re.search(r'FREEDESKTOP_VERSION:\s*"([^"]+)"', text)
    if not m or m.group(1) != str(manifest.get("runtime-version")):
        errs.append("rc workflow: FREEDESKTOP_VERSION must equal the manifest's runtime-version")
    errs += _builder_host_tools(text, "rc workflow")
    return errs


def _triggers(doc):
    on = doc.get("on", doc.get(True))  # YAML 1.1 reads the key `on` as the boolean True
    if isinstance(on, str):
        return {on: None}
    if isinstance(on, list):
        return {k: None for k in on}
    return on or {}


OWNER_GUARD_PARTS = ("github.actor == github.repository_owner", "github.triggering_actor == github.repository_owner", "github.event_name == 'push'", "github.ref_type == 'tag'")
RELEASE_ASSETS = ("tsw-forever-${version}.flatpak", "tsw-forever-${version}-src.tar.gz", "tsw-forever-${version}-windows-setup.exe", "dist/SHA256SUMS")


def check_tag_release_workflow(doc):
    """The single tag-triggered release workflow (release.yml): started only by an owner-pushed numeric version tag, it builds the combined set through
    release-set.yml from the tagged commit and publishes exactly four files through a draft. Only the publish job may write; no secrets, no inputs."""
    errs = []
    trig = _triggers(doc)
    text = repr(doc)
    if list(trig) != ["push"] or list((trig.get("push") or {})) != ["tags"] or list((trig["push"]["tags"])) != ["v*.*.*"]:
        errs.append("release.yml: the only trigger is push of tags v*.*.* (no branch, no manual start, no free input)")
    if "write" in repr(doc.get("permissions")) or str((doc.get("permissions") or {})) != "{'contents': 'read'}":
        errs.append("release.yml: the workflow-level permissions must be exactly contents: read")
    if re.search(r"secrets\.|git push|git tag|create-release|action-gh-release", text):
        errs.append("release.yml: no secrets, no push, no tag creation, no third-party release action")
    jobs = doc.get("jobs") or {}
    if sorted(jobs) != ["guard", "publish", "set"]:
        errs.append(f"release.yml: the jobs are exactly guard, set and publish (found {sorted(jobs)})")
        return errs
    for jname, job in jobs.items():
        cond = str(job.get("if", ""))
        for part in OWNER_GUARD_PARTS:
            if part not in cond:
                errs.append(f"release.yml: job {jname} lacks the guard part {part!r} (owner-pushed tag only)")
        if jname != "publish" and "write" in repr(job.get("permissions")):
            errs.append(f"release.yml: job {jname} must not have write permissions")
    if jobs["set"].get("uses") != "./.github/workflows/release-set.yml":
        errs.append("release.yml: the set job must call ./.github/workflows/release-set.yml (one build path for dry run and release)")
    pub = jobs["publish"]
    if sorted(pub.get("needs") or []) != ["guard", "set"]:
        errs.append("release.yml: publish must need guard and set (a failed platform build must stop the release)")
    if "write" not in repr((pub.get("permissions") or {}).get("contents")):
        errs.append("release.yml: the publish job needs contents: write")
    ptext = repr(pub)
    for need, why in (
        ("merge-base --is-ancestor", "check that the tagged commit is part of main"),
        ("gh release view", "refuse an existing release"),
        ("--draft --verify-tag", "create the release as a draft for an existing tag"),
        ("gh release download", "download the uploaded assets again"),
        ("sha256sum --check", "verify the checksums"),
        ("--draft=false", "publish only after the verification"),
        ("check_release_set.py", "re-check the exact release set"),
        ("check_public_source.py", "re-audit the source archive"),
        ("persist-credentials", "check out without stored credentials"),
    ):
        if need not in ptext:
            errs.append(f"release.yml: publish must {why} ({need!r} missing)")
    for asset in RELEASE_ASSETS:
        if asset not in ptext:
            errs.append(f"release.yml: the release asset {asset} is not uploaded")
    if ptext.count("-eq 4") < 2:
        errs.append("release.yml: the release has exactly four assets (checked before the upload and after the download)")
    if "--clobber" in ptext or "gh release delete" in ptext:
        errs.append("release.yml: an existing release or asset is never replaced or deleted")
    if "--draft=false" in ptext and "gh release download" in ptext and ptext.index("--draft=false") < ptext.index("gh release download"):
        errs.append("release.yml: the release is made public only after the download check")
    for jname, job in jobs.items():
        for st in job.get("steps") or []:
            if "${{ github.event" in str(st.get("run", "")) or "${{ github.ref_name" in str(st.get("run", "")):
                errs.append(f"release.yml: job {jname}: context values reach scripts through env:, never interpolated into a shell script")
    return errs


def check_ci_policy(workflows, manifest):
    """The build policy for ALL workflows: branches and pull requests are checked only; a full package or publication is possible from main only,
    cannot be started for another ref or a foreign commit, and the release has no free input. `workflows` = {file name: parsed YAML}."""
    import yaml  # noqa: F401  (imported by the caller; kept here to fail loudly when missing)
    errs = []
    names = set(workflows)
    for need in ("ci.yml", "flatpak.yml", "release-flatpak.yml", "rc-artifacts.yml", "windows.yml", "release-set.yml", "release.yml"):
        if need not in names:
            errs.append(f"missing workflow {need}")
    for name, doc in sorted(workflows.items()):
        trig = _triggers(doc)
        text = repr(doc)
        builds = bool(BUILD_MARKERS.search(text))
        for bad in ("pull_request_target", "workflow_run", "schedule", "release", "registry_package"):
            if bad in trig:
                errs.append(f"{name}: trigger {bad} is not allowed")
        if name == "release.yml":
            errs += check_tag_release_workflow(doc)
            continue
        push = trig.get("push") or {}
        if "push" in trig and (push.get("tags") or push.get("tags-ignore")):
            errs.append(f"{name}: no tag triggers (a tag must not start a build or a release)")
        if name == "ci.yml":
            if builds:
                errs.append("ci.yml: the check workflow must not build or upload a package, repository or site")
            if "secrets." in text:
                errs.append("ci.yml: the check workflow has no secrets")
            continue
        # every other workflow builds or publishes: main only
        if "pull_request" in trig:
            errs.append(f"{name}: a package workflow must not run for pull requests")
        if "push" in trig and list(push.get("branches", [])) != ["main"]:
            errs.append(f"{name}: push must be limited to branches: [main]")
        for jname, job in (doc.get("jobs") or {}).items():
            cond = str(job.get("if", ""))
            if MAIN_GUARD not in cond:
                errs.append(f"{name}: job {jname} lacks the job-level guard {MAIN_GUARD!r} (a manual run from another branch would bypass the trigger filter)")
            steps = job.get("steps") or []
            if any("actions/checkout" in str(st.get("uses", "")) for st in steps):
                if not any("merge-base --is-ancestor" in str(st.get("run", "")) and "origin/main" in str(st.get("run", "")) for st in steps):
                    errs.append(f"{name}: job {jname} checks out the repository but does not verify that the commit is part of main")
                for st in steps:
                    if "actions/checkout" in str(st.get("uses", "")) and (st.get("with") or {}).get("fetch-depth") != 0:
                        errs.append(f"{name}: job {jname}: the checkout needs fetch-depth: 0 for the main check")
        if name == "release-flatpak.yml":
            if list(trig) != ["workflow_dispatch"]:
                errs.append("release-flatpak.yml: manual start (workflow_dispatch) only")
            inputs = (trig.get("workflow_dispatch") or {}).get("inputs") or {}
            if sorted(inputs) != ["channel", "confirm"]:
                errs.append(f"release-flatpak.yml: the only inputs are confirm and channel (found {sorted(inputs)}): no free ref, SHA or version")
            ch = inputs.get("channel") or {}
            if ch.get("type") != "choice" or sorted(ch.get("options") or []) != ["stable", "testing"]:
                errs.append("release-flatpak.yml: channel must be a choice of exactly stable and testing")
            for jname, job in (doc.get("jobs") or {}).items():
                for st in job.get("steps") or []:
                    if "inputs." in str(st.get("run", "")) or "github.event.inputs" in str(st.get("run", "")):
                        errs.append(f"release-flatpak.yml: job {jname}: inputs must reach scripts through env:, never be interpolated into a shell script")
            if "inputs.confirm == 'PUBLISH'" not in repr(doc):
                errs.append("release-flatpak.yml: the typed confirmation is missing")
        elif name == "flatpak.yml":
            if "pages" in repr(doc.get("permissions")) or "secrets." in text:
                errs.append("flatpak.yml: the build workflow has no publishing rights and no secrets")
        elif name == "windows.yml":
            if sorted(trig) != ["workflow_call", "workflow_dispatch"]:
                errs.append("windows.yml: manual start (workflow_dispatch) or a call from the release-set workflow only: a push, a tag or a schedule must not start it")
            if (trig.get("workflow_dispatch") or {}).get("inputs"):
                errs.append("windows.yml: no inputs (no free ref, SHA or version)")
            if "secrets." in text or "write" in repr(doc.get("permissions")):
                errs.append("windows.yml: no secrets and no write permission (unsigned test build, nothing is published)")
            for jname, job in (doc.get("jobs") or {}).items():
                if "write" in repr(job.get("permissions")):
                    errs.append(f"windows.yml: job {jname} must not have write permissions")
                if not str(job.get("runs-on", "")).startswith("windows-"):
                    errs.append(f"windows.yml: job {jname} must run on a Windows runner")
        elif name == "release-set.yml":
            if sorted(trig) != ["workflow_call", "workflow_dispatch"] or (trig.get("workflow_dispatch") or {}).get("inputs"):
                errs.append("release-set.yml: manual start (workflow_dispatch, no inputs) or a call from release.yml only: a push or a tag must not start it")
            if "secrets." in text or "write" in repr(doc.get("permissions")) or re.search(r"gh release|create-release|action-gh-release|git push|git tag", text):
                errs.append("release-set.yml: no secrets, no write permission, no release, tag or push (the combined set is a workflow artifact only)")
            jobs = doc.get("jobs") or {}
            asm = jobs.get("assemble") or {}
            if sorted(asm.get("needs") or []) != ["linux", "windows"]:
                errs.append("release-set.yml: the assemble job must need exactly the linux and windows jobs (a failed platform must stop the set)")
            if "needs.linux.result" not in repr(asm) or "needs.windows.result" not in repr(asm):
                errs.append("release-set.yml: the assemble job must check that both platform results are success")
            if "--with-windows" not in repr(asm):
                errs.append("release-set.yml: the combined set must be verified with check_release_set.py --with-windows")
        elif name == "rc-artifacts.yml":
            if sorted(trig) != ["workflow_call", "workflow_dispatch"]:
                errs.append("rc-artifacts.yml: manual start (workflow_dispatch) or a call from release-set.yml only: a tag or a push must not start it")
            if (trig.get("workflow_dispatch") or {}).get("inputs"):
                errs.append("rc-artifacts.yml: no inputs (no free ref, SHA or version)")
            if "secrets." in text or "write" in repr(doc.get("permissions")):
                errs.append("rc-artifacts.yml: no secrets and no write permission")
            for jname, job in (doc.get("jobs") or {}).items():
                if "refs/tags/v" not in str(job.get("if", "")):
                    errs.append(f"rc-artifacts.yml: job {jname}: the guard must allow main or a v* tag explicitly")
                if "write" in repr(job.get("permissions")):
                    errs.append(f"rc-artifacts.yml: job {jname} must not have write permissions")
    return errs



def main(argv):
    if len(argv) == 4 and argv[1] == "ci-policy":
        import yaml
        wf = {}
        for f in sorted(os.listdir(argv[2])):
            if f.endswith((".yml", ".yaml")):
                wf[f] = yaml.safe_load(open(os.path.join(argv[2], f)))
        errs = check_ci_policy(wf, json.load(open(argv[3])))
        for e in errs:
            print("VIOLATION:", e)
        print("OK" if not errs else f"{len(errs)} violation(s)")
        return 1 if errs else 0
    if len(argv) == 4 and argv[1] == "release-workflow":
        errs = check_release_workflow(open(argv[2]).read(), json.load(open(argv[3])))
        for e in errs:
            print("VIOLATION:", e)
        print("OK" if not errs else f"{len(errs)} violation(s)")
        return 1 if errs else 0
    if len(argv) == 4 and argv[1] == "rc-workflow":
        errs = check_rc_workflow(open(argv[2]).read(), json.load(open(argv[3])))
        for e in errs:
            print("VIOLATION:", e)
        print("OK" if not errs else f"{len(errs)} violation(s)")
        return 1 if errs else 0
    if len(argv) == 4 and argv[1] == "workflow":
        errs = check_workflow(open(argv[2]).read(), json.load(open(argv[3])))
        for e in errs:
            print("VIOLATION:", e)
        print("OK" if not errs else f"{len(errs)} violation(s)")
        return 1 if errs else 0
    if len(argv) != 3 or argv[1] not in ("manifest", "tree"):
        print(__doc__)
        return 2
    errs = check_manifest(json.load(open(argv[2]))) if argv[1] == "manifest" else check_tree(argv[2])
    for e in errs:
        print("VIOLATION:", e)
    print("OK" if not errs else f"{len(errs)} violation(s)")
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
