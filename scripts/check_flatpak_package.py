#!/usr/bin/env python3
"""Policy checks for the Flatpak package, run before any build (no network, no Flatpak needed).

  check_flatpak_package.py manifest <manifest.json>   sandbox permissions, sources and build commands
  check_flatpak_package.py tree <stage-dir>           the staged source tree contains no game data and nothing private
  check_flatpak_package.py workflow <workflow.yml> <manifest.json>   the CI definition follows the project rules
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
    builds = re.search(r"build_flatpak\.sh\s+(build|bundle|all)\b", text) or any("flatpak-builder" in l for l in lines)
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


BUILD_MARKERS = re.compile(r"flatpak-builder|build_flatpak\.sh\s+(build|bundle|all|vendor)\b|flatpak\s+build-|actions/upload-artifact|upload-pages-artifact|deploy-pages|ostree\s+(commit|pull)")
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
        (r"deploy-pages|upload-pages-artifact", "nothing is published from this workflow"),
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


RELEASE_JOBS = ["approve", "candidate", "draft", "guard", "publish"]
NEEDS = {  # job: the jobs it must (at least) wait for; the chain is what keeps anything public behind both approvals
    "candidate": ["guard"], "approve": ["guard", "candidate"], "draft": ["candidate", "approve"], "publish": ["draft"],
}


def _needs(job):
    n = job.get("needs")
    return [n] if isinstance(n, str) else list(n or [])


def _perms(job):
    return dict(job.get("permissions") or {})


def check_tag_release_workflow(doc):
    """The single tag-triggered release PIPELINE (release.yml): started only by an owner-pushed numeric version tag. It compiles nothing, verifies the candidate that build.yml built for
    exactly that commit, waits for the owner's approval of the whole release (environment `flatpak-release`, approval 1), fills a DRAFT release and publishes the draft only in the job
    behind the environment `release-publish` (approval 2). It uses no secret, no variable and no key; write permissions exist only where they are needed; nothing is chained by
    another workflow; GitHub Pages is not touched."""
    errs = []
    trig = _triggers(doc)
    text = repr(doc)
    if list(trig) != ["push"] or list((trig.get("push") or {})) != ["tags"] or list((trig["push"]["tags"])) != ["v*.*.*"]:
        errs.append("release.yml: the only trigger is push of tags v*.*.* (no branch, no manual start, no free input, no workflow_run)")
    if str((doc.get("permissions") or {})) != "{'contents': 'read'}":
        errs.append("release.yml: the workflow-level permissions must be exactly contents: read")
    if re.search(r"git push|git tag|create-release|action-gh-release|secrets:\s*inherit|'secrets': 'inherit'|pull_request", text):
        errs.append("release.yml: no push, no tag creation, no third-party release action, no inherited secrets")
    if re.search(r"secrets\.|vars\.", text):
        errs.append("release.yml: no secret and no variable (there is no signing key any more)")
    if re.search(r"gpg|--gpg-sign|flatpak_sign|flatpak_keys|ostree|deploy-pages|upload-pages-artifact|pages-build", text):
        errs.append("release.yml: no signing, no OSTree and no GitHub Pages deployment (Pages holds the static web site only; build.yml deploys it)")
    jobs = doc.get("jobs") or {}
    if sorted(jobs) != RELEASE_JOBS:
        errs.append(f"release.yml: the jobs are exactly {', '.join(RELEASE_JOBS)} (found {sorted(jobs)})")
        return errs
    for jname, job in jobs.items():
        cond = str(job.get("if", ""))
        for part in OWNER_GUARD_PARTS:
            if part not in cond:
                errs.append(f"release.yml: job {jname} lacks the guard part {part!r} (owner-pushed tag only)")
        if jname not in ("approve", "publish") and "environment" in job:
            errs.append(f"release.yml: job {jname} must not use an environment (only approve and publish do)")
        for dep in NEEDS.get(jname, []):
            if dep not in _needs(job):
                errs.append(f"release.yml: job {jname} must need {dep}")
        perms = _perms(job)
        if jname in ("draft", "publish"):
            if perms != {"contents": "write"}:
                errs.append(f"release.yml: job {jname} needs exactly contents: write (found {perms})")
        elif any(v == "write" for v in perms.values()):
            errs.append(f"release.yml: job {jname} must not have write permissions")
        for st in job.get("steps") or []:
            run = str(st.get("run", ""))
            if "${{ github.event" in run or "${{ github.ref_name" in run:
                errs.append(f"release.yml: job {jname}: context values reach scripts through env:, never interpolated into a shell script")
    # nothing is built at the tag: the candidate of the build run of exactly this commit is verified and taken over (build.yml built it when the commit was pushed to main)
    if re.search(r"flatpak-stable-build|windows\.yml|build_flatpak\.sh (build|bundle|vendor|test)|cargo (build|test)|flatpak-builder", text):
        errs.append("release.yml: the release tag compiles and tests nothing (the candidate was built by build.yml when the commit was pushed)")
    cand = jobs["candidate"]
    ctext = repr(cand)
    for need, why in (("--workflow build.yml --commit", "look for the build run of exactly the tagged commit"), ("--status success", "accept a successful run only"), ("expired == false", "refuse expired artifacts"),
                      ("release_candidate.py verify", "verify every file against candidate.json"), ("run-id", "download from that build run"), ("--version", "check the version")):
        if need not in ctext:
            errs.append(f"release.yml: job candidate must {why} ({need!r} missing)")
    if _perms(cand) != {"contents": "read", "actions": "read"}:
        errs.append("release.yml: job candidate has exactly contents: read and actions: read")
    if "environment" in cand:
        errs.append("release.yml: job candidate uses no environment")
    # the two human approvals: the environments with their required reviewers. Neither may be removed, renamed or bypassed.
    approve = jobs["approve"]
    if approve.get("environment") != "flatpak-release":
        errs.append("release.yml: the job approve uses the environment flatpak-release (approval 1 of the whole release) and nothing else")
    if any("uses" in st for st in approve.get("steps") or []) or re.search(r"scripts/|secrets\.|checkout", repr(approve)):
        errs.append("release.yml: the job approve runs no action and no project code: it only waits for the approval")
    pub = jobs["publish"]
    if pub.get("environment") != "release-publish":
        errs.append("release.yml: the job publish uses the environment release-publish (approval 2)")
    if "approve" not in _needs(jobs["draft"]):
        errs.append("release.yml: nothing is put into a draft before the approval (draft needs approve)")
    # publication: only through the script, only in the job behind approval 2
    for jname, job in jobs.items():
        for st in job.get("steps") or []:
            run = str(st.get("run", ""))
            if re.search(r"gh\s+release\s+(create|edit|upload|delete)|--draft=false", run):
                errs.append(f"release.yml: job {jname}: no direct gh release command: the release is handled only by scripts/release_draft.sh")
            if "release_draft.sh publish" in run and jname != "publish":
                errs.append(f"release.yml: job {jname}: only the job publish may publish")
            if "release_draft.sh upload" in run and jname != "draft":
                errs.append(f"release.yml: job {jname}: only the job draft may fill the draft")
    ptext = repr(pub)
    if "scripts/release_draft.sh publish" not in ptext or "persist-credentials" not in ptext:
        errs.append("release.yml: publish runs scripts/release_draft.sh publish and checks out without stored credentials")
    for need, jn, why in (
        ("merge-base --is-ancestor", "guard", "check that the tagged commit is part of main"), ("pin_actions.sh --check", "guard", "refuse unpinned actions"),
        ("version.py compare", "guard", "refuse a version that is not newer than the newest published release"),
        ("check_release_set.py", "draft", "check the exact release set"), ("--with-windows", "draft", "include the Windows installer in the set"),
        ("check_public_source.py", "draft", "audit the source archive"), ("sha256sum -c SHA256SUMS-flatpak", "draft", "check the checksum of the bundle"), ("scripts/release_draft.sh upload", "draft", "fill the draft and read it back"),
    ):
        if need not in repr(jobs[jn]):
            errs.append(f"release.yml: job {jn} must {why} ({need!r} missing)")
    return errs


# The Flatpak tests and the release script tests need these programs. A job that runs `build_flatpak.sh test` must install them BEFORE the step and set TSW_REQUIRE_TOOLS=1
# (a missing program is then a failure; without it the tests are skipped silently, which once let 38 of them pass without running).
TEST_PACKAGES = ("flatpak",)
E2E_PACKAGES = ("flatpak",)


def _apt_packages(run_texts):
    """The package names of every `apt-get install` command in the given `run` texts."""
    names = set()
    for text in run_texts:
        for m in re.finditer(r"apt-get\s+install([^\n]*)", text):
            names.update(t for t in m.group(1).split() if not t.startswith("-"))
    return names


def _required_tool_rules(name, doc):
    """Every step that runs `scripts/build_flatpak.sh test` or the Flatpak end-to-end test must find its programs installed by an EARLIER step of the same job, and the test step must set
    TSW_REQUIRE_TOOLS to "1"."""
    errs = []
    for jname, job in (doc.get("jobs") or {}).items():
        steps = job.get("steps") or []
        for i, st in enumerate(steps):
            run = str(st.get("run", ""))
            before = _apt_packages(str(x.get("run", "")) for x in steps[:i])
            if re.search(r"scripts/build_flatpak\.sh\s+test\b", run):
                if str((st.get("env") or {}).get("TSW_REQUIRE_TOOLS")) != "1":
                    errs.append(f"{name}: job {jname}: the step that runs `build_flatpak.sh test` must set TSW_REQUIRE_TOOLS: \"1\" (without it the Flatpak and release tests are skipped silently when a program is missing)")
                gone = [p for p in TEST_PACKAGES if p not in before]
                if gone:
                    errs.append(f"{name}: job {jname}: the tests need {', '.join(gone)}: install them with apt-get in an earlier step of the job")
            if "scripts/e2e_flatpak_bundle.sh" in run:
                gone = [p for p in E2E_PACKAGES if p not in before]
                if gone:
                    errs.append(f"{name}: job {jname}: the end-to-end test needs {', '.join(gone)}: install them with apt-get in an earlier step of the job")
    return errs


def check_ci_policy(workflows, manifest):
    """The build policy for ALL workflows: branches and pull requests are checked only; a full package or publication is possible from main only,
    cannot be started for another ref or a foreign commit, and the release has no free input. `workflows` = {file name: parsed YAML}."""
    import yaml  # noqa: F401  (imported by the caller; kept here to fail loudly when missing)
    errs = []
    names = set(workflows)
    for need in ("ci.yml", "flatpak.yml", "flatpak-stable-build.yml", "rc-artifacts.yml", "windows.yml", "release-set.yml", "release.yml", "build.yml"):
        if need not in names:
            errs.append(f"missing workflow {need}")
    for name, doc in sorted(workflows.items()):
        trig = _triggers(doc)
        text = repr(doc)
        builds = bool(BUILD_MARKERS.search(text))
        errs += _required_tool_rules(name, doc)
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
            # ci.yml may CALL the package build and the Windows build, and only (1) after all of its own checks (needs: checks, the tests of the same commit in the same run)
            # and (2) on main (job-level guard): a pull request or another branch never reaches a package build
            # fail-safe: an optional part is skipped only on an explicit `false` of the decision step; "== 'true'" would skip it when the step breaks
            for jname, job in (doc.get("jobs") or {}).items():
                conds = [str(job.get("if", ""))] + [str(st.get("if", "")) for st in job.get("steps") or []]
                for cnd in conds:
                    if re.search(r"outputs\.(rust|e2e|flatpak)\s*==\s*'true'", cnd):
                        errs.append(f"ci.yml: job {jname}: an optional part must be skipped only on an explicit 'false' (use != 'false'), never run only on 'true'")
            for jname, job in (doc.get("jobs") or {}).items():
                used = str(job.get("uses", ""))
                if not used:
                    continue
                if used not in ("./.github/workflows/flatpak.yml", "./.github/workflows/windows.yml"):
                    errs.append(f"ci.yml: job {jname} calls {used}: only flatpak.yml and windows.yml may be called")
                needs = job.get("needs")
                if "checks" not in ([needs] if isinstance(needs, str) else list(needs or [])):
                    errs.append(f"ci.yml: job {jname} calls {used} without `needs: checks` (a package or Windows build must follow the green checks of the same run)")
                if MAIN_GUARD not in str(job.get("if", "")):
                    errs.append(f"ci.yml: job {jname} calls {used} without the main guard {MAIN_GUARD!r}")
                if "write" in repr(job.get("permissions")) or "secrets" in repr(job):
                    errs.append(f"ci.yml: job {jname} must not have write permissions or secrets")
                if used.endswith("/windows.yml") and (job.get("with") or {}).get("use_cache") is not True:
                    errs.append(f"ci.yml: job {jname} must call windows.yml with use_cache: true explicitly (the development check is the only caller that may use the cache)")
            continue
        # a development cache is never used by a workflow that builds a release or a release candidate; windows.yml (also called by the release workflows) may restore one only
        # when it was started directly or by ci.yml, never from release-set.yml or release.yml (`github.workflow` names the CALLER's workflow)
        if name in ("rc-artifacts.yml", "release-set.yml", "release.yml", "flatpak-stable-build.yml") and "actions/cache" in text:
            errs.append(f"{name}: a release build must not use a cache (clean, independent builds)")
        if "TSW_RELEASE_BUILD" in text and name != "flatpak-stable-build.yml":
            errs.append(f"{name}: only flatpak-stable-build.yml makes a release build (TSW_RELEASE_BUILD); development and candidate builds must not")
        if name == "windows.yml":
            # the cache is opt-in: an input that defaults to false, set to true only by ci.yml and given explicitly as false by the release workflows (checked below)
            call_inputs = ((trig.get("workflow_call") or {}).get("inputs") or {})
            uc = call_inputs.get("use_cache") or {}
            if uc.get("type") != "boolean" or uc.get("default") is not False:
                errs.append("windows.yml: workflow_call needs the boolean input use_cache with the default false")
            for jname, job in (doc.get("jobs") or {}).items():
                for st in job.get("steps") or []:
                    if "actions/cache" in str(st.get("uses", "")):
                        cond = str(st.get("if", ""))
                        if "inputs.use_cache == true" not in cond:
                            errs.append(f"windows.yml: job {jname}: the cache step must be opt-in (inputs.use_cache == true)")
                        if "release" in cond or re.search(r"github\.workflow\s*==\s*'(?!windows')", cond):
                            errs.append(f"windows.yml: job {jname}: the cache step must never be enabled for a release workflow")
        # a job that calls another workflow passes no secret on, has no write permission, and calls windows.yml with an EXPLICIT use_cache: only ci.yml (a development check) may
        # say true, every other caller (the release workflows) must say false
        for jname, job in (doc.get("jobs") or {}).items():
            used = str(job.get("uses", ""))
            if not used:
                continue
            if "secrets" in job:
                errs.append(f"{name}: job {jname} passes secrets to {used} (no workflow in this project uses secrets)")
            if "write" in repr(job.get("permissions")):
                errs.append(f"{name}: job {jname} calls {used} with write permissions")
            if used.endswith("/windows.yml"):
                want = name == "ci.yml"
                got = (job.get("with") or {}).get("use_cache")
                if got is not want:
                    errs.append(f"{name}: job {jname} must call windows.yml with use_cache: {str(want).lower()} explicitly (found {got!r}): the Windows cache is for development runs only")
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
        if name == "flatpak-stable-build.yml":
            if list(trig) != ["workflow_call"]:
                errs.append("flatpak-stable-build.yml: it is only called (workflow_call) by build.yml, never started by a push, a tag or by hand")
            if "secrets." in text or "vars." in text or "write" in repr(doc.get("permissions")) or "'environment':" in text:
                errs.append("flatpak-stable-build.yml: no secret, no variable, no environment, no write permission")
            if re.search(r"gpg\s+--(batch\s+--)?import|--gpg-sign|--gpg-keys|--repo-url|flatpak_sign|flatpak_keys|secret-keys?|gh\s+release|deploy-pages|upload-pages-artifact", text):
                errs.append("flatpak-stable-build.yml: it builds the plain bundle (no key, no update source) and neither signs nor publishes")
            for jname, job in (doc.get("jobs") or {}).items():
                if "write" in repr(job.get("permissions")):
                    errs.append(f"flatpak-stable-build.yml: job {jname} must not have write permissions")
            # the bundle keeps the branch of the earlier bundles (`local`) so that an installed version is updated in place; a RELEASE is told from a developer build by build metadata
            if re.search(r"FLATPAK_BRANCH['\"]?\s*[:=]\s*['\"]?(stable|testing)|//stable|x86_64/stable|stable/active", text):
                errs.append("flatpak-stable-build.yml: the release bundle is the branch local (no FLATPAK_BRANCH stable or testing, no //stable): an earlier installation must be updated in place, not get a parallel branch")
            for step_name, needle in (("build", "build_flatpak.sh build"), ("bundle", "build_flatpak.sh bundle")):
                hits = [st for j in (doc.get("jobs") or {}).values() for st in j.get("steps") or [] if needle in str(st.get("run", ""))]
                if not hits or any(str((st.get("env") or {}).get("TSW_RELEASE_BUILD")) != "1" for st in hits):
                    errs.append(f"flatpak-stable-build.yml: the {step_name} step must set TSW_RELEASE_BUILD: \"1\" (the release build metadata)")
            if "tsw-forever-release-build" not in text or "//local" not in text:
                errs.append("flatpak-stable-build.yml: the audit must check the release build marker in both programs and use the branch local")
        elif name == "build.yml":
            # BUILD ON PUSH: candidate artifacts and the web site; nothing that publishes a release or activates a Flatpak update, no key
            if sorted(trig) != ["push", "workflow_dispatch"] or (trig.get("workflow_dispatch") or {}).get("inputs"):
                errs.append("build.yml: the triggers are push to main and a manual start without inputs")
            if "secrets." in text or "vars." in text or "flatpak-release" in text or "release-publish" in text:
                errs.append("build.yml: no secret, no variable and no release environment: the build of the candidate never meets an approval or a secret")
            if re.search(r"gpg\s+--(batch\s+--)?import|--gpg-sign|flatpak_sign_ci|secret-keys?|gh\s+release|release_draft|git\s+(push|tag|commit)", text):
                errs.append("build.yml: it signs nothing, creates no release, no tag and no commit")
            jobs = doc.get("jobs") or {}
            if sorted(jobs) != ["candidate", "flatpak", "site", "windows"]:
                errs.append(f"build.yml: the jobs are exactly candidate, flatpak, site, windows (found {sorted(jobs)})")
            else:
                if jobs["flatpak"].get("uses") != "./.github/workflows/flatpak-stable-build.yml":
                    errs.append("build.yml: the flatpak job calls flatpak-stable-build.yml")
                site = jobs["site"]
                if _needs(site):
                    errs.append("build.yml: the web site does not wait for the builds (it is independent of any release)")
                if _perms(site) != {"contents": "read", "pages": "write", "id-token": "write"}:
                    errs.append(f"build.yml: job site has exactly contents: read, pages: write, id-token: write (found {_perms(site)})")
                if (site.get("concurrency") or {}).get("group") != "pages-site" or (site.get("concurrency") or {}).get("cancel-in-progress") is not False:
                    errs.append("build.yml: job site shares the concurrency group pages-site (no cancel): web site and release deployments never interleave")
                if "github.event_name == 'push'" not in str(site.get("if", "")):
                    errs.append("build.yml: the site is deployed for a push to main only")
                stext = repr(site)
                for need, why in (("assemble_pages_site.py --website website", "assemble the static web site only"), ("test_website.py", "test the web site before it goes live"),
                                  ("git ls-remote", "deploy only the tip of main"), ("deploy-pages", "deploy")):
                    if need not in stext:
                        errs.append(f"build.yml: job site must {why} ({need!r} missing)")
                if re.search(r"--repo\b|--refs\b|preserve_live|channels\.json|flatpakref|flatpakrepo|\.flatpak\b|build/flatpak|/repo\b|OSTree|ostree", stext):
                    errs.append("build.yml: GitHub Pages carries the static web site only: no repository, no install file, no release file")
                for jn in ("flatpak", "windows", "candidate"):
                    if any(v == "write" for v in _perms(jobs[jn]).values()):
                        errs.append(f"build.yml: job {jn} must not have write permissions")
                for jn, job in jobs.items():
                    if jn != "site" and "deploy-pages" in repr(job):
                        errs.append(f"build.yml: job {jn} must not deploy")
                cand = repr(jobs["candidate"])
                for need in ("release_candidate.py create", "'retention-days': 30"):
                    if need not in cand:
                        errs.append(f"build.yml: job candidate must write the manifest and keep it 30 days ({need!r} missing)")
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
