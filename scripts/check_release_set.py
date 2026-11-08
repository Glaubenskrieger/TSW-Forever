#!/usr/bin/env python3
"""Consistency of a release set: the official Linux artifacts are the Flatpak bundle and the source archive, nothing else.

  check_release_set.py <folder> [--version X.Y.Z[-pre]] [--with-windows]

<folder> must contain EXACTLY these three files (names derive from the version, `packaging/project.conf` APP_VERSION when --version is not given):
  tsw-forever-<V>.flatpak   tsw-forever-<V>-src.tar.gz   SHA256SUMS
and it checks that
  - no other file is there (distribution-specific formats are not offered)
  - SHA256SUMS lists exactly the two artifacts and every listed hash is the real hash (sha256sum -c semantics, no extra or missing entries)
  - the Flatpak bundle is not empty
  - the source archive has the single top folder tsw-forever-<V>/, packaging/project.conf inside says APP_VERSION=<V>, and it carries everything needed to build
    the launcher on your own (REQUIRED_IN_SOURCE: the three Cargo.lock files, the window definition, the licence, the build instructions ...)
With --with-windows the set is the combined multi-platform release and must also contain, listed in SHA256SUMS with real hashes,
  tsw-forever-<V>-windows-setup.exe   (the only Windows artifact: there is no portable zip; same version as the Flatpak and the source archive)
Exit code 0 = consistent, 1 = at least one mismatch (printed), 2 = usage error. Nothing is written.
"""
import hashlib
import os
import re
import sys
import tarfile
from pathlib import Path
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import check_licenses

ROOT = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..")

# What a person needs to build the launcher from the archive alone (paths relative to the top folder of the archive).
REQUIRED_IN_SOURCE = (
    "LICENSE", "THIRD-PARTY-NOTICES.md", "README.md", "CHANGELOG.md", "rustfmt.toml", ".cargo/config.toml",
    "documentation/building-from-source.md", "documentation/user-guide.md", "documentation/public-export.md", "documentation/microsoft-components.md",
    "documentation/architecture.md", "documentation/packaging.md", "documentation/security.md", "documentation/third-party-components.md", "documentation/versioning-policy.md",
    "LICENSES/inventory.json", "LICENSES/README.md", "scripts/check_licenses.py",
    "packaging/project.conf", "packaging/flatpak/org.tswlauncher.Launcher.metainfo.xml",
    "scripts/version.py", "scripts/install_native.sh",
    "patcher/Cargo.toml", "patcher/Cargo.lock", "patcher/src/lib.rs",
    "launcher/Cargo.toml", "launcher/Cargo.lock", "launcher/src/lib.rs", "launcher/src/main.rs",
    "launcher-gui/Cargo.toml", "launcher-gui/Cargo.lock", "launcher-gui/build.rs", "launcher-gui/src/main.rs", "launcher-gui/ui/launcher.slint",
    "launcher-gui/translations/de/LC_MESSAGES/tsw-launcher-gui.po", "launcher-gui/assets/icon.svg",
    "launcher-gui/packaging/linux/org.tswlauncher.Launcher.desktop",
    "packaging/windows/tsw-forever.iss", "scripts/build_windows_installer.ps1", "documentation/building-windows.md",
)


def sha256(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def parse_sums(text):
    out = {}
    for line in text.splitlines():
        m = re.fullmatch(r"([0-9a-f]{64})  \*?(.+)", line.strip())
        if m:
            out[m.group(2)] = m.group(1)
    return out


def app_version():
    for line in open(os.path.join(ROOT, "packaging", "project.conf")):
        if line.startswith("APP_VERSION="):
            return line.split("=", 1)[1].strip()
    raise SystemExit("no APP_VERSION in packaging/project.conf")


def expected_names(version, windows=False):
    names = {"flatpak": f"tsw-forever-{version}.flatpak", "src": f"tsw-forever-{version}-src.tar.gz", "sums": "SHA256SUMS"}
    if windows:
        names.update({"windows-setup": f"tsw-forever-{version}-windows-setup.exe"})
    return names


def check(folder, version, windows=False):
    errs = []
    names = expected_names(version, windows)
    for kind, n in names.items():
        if not os.path.isfile(os.path.join(folder, n)):
            errs.append(f"missing {kind}: {n}")
    if errs:
        return errs
    # nothing else is part of a release
    present = sorted(os.listdir(folder))
    for n in present:
        if n not in names.values():
            errs.append(f"unexpected file in the release set: {n} (the release consists of the Flatpak bundle, the source archive, the Windows installer when requested, and SHA256SUMS)")
    # checksums
    sums = parse_sums(open(os.path.join(folder, names["sums"])).read())
    listed = {n for k, n in names.items() if k != "sums"}
    for n in sorted(listed):
        if n not in sums:
            errs.append(f"SHA256SUMS does not list {n}")
        elif sums[n] != sha256(os.path.join(folder, n)):
            errs.append(f"SHA256SUMS: wrong hash for {n}")
    for n in sums:
        if n not in listed:
            errs.append(f"SHA256SUMS lists a file that is not an artifact of the release: {n}")
    if os.path.getsize(os.path.join(folder, names["flatpak"])) < 1024 * 1024:
        errs.append("the Flatpak bundle is suspiciously small (under 1 MiB)")
    if windows:
        with open(os.path.join(folder, names["windows-setup"]), "rb") as f:
            if f.read(2) != b"MZ":
                errs.append("the Windows installer is not a Windows executable (no MZ header)")
        if os.path.getsize(os.path.join(folder, names["windows-setup"])) < 512 * 1024:
            errs.append("the Windows installer is suspiciously small (under 512 KiB)")
    # source archive
    with tarfile.open(os.path.join(folder, names["src"]), "r:gz") as t:
        members = {m.name for m in t.getmembers()}
        tops = {n.split("/")[0] for n in members}
        top = f"tsw-forever-{version}"
        if tops != {top}:
            errs.append(f"source archive top folder(s) {sorted(tops)}, expected {top}")
        for rel in REQUIRED_IN_SOURCE:
            if f"{top}/{rel}" not in members:
                errs.append(f"source archive lacks {rel} (needed to build on your own)")
        def read_source(rel):
            f = t.extractfile(f"{top}/{rel}")
            if f is None:
                raise ValueError(f"source licence file is not regular: {rel}")
            return f.read()
        errs.extend(check_licenses.check(read_source))
        try:
            conf = t.extractfile(f"{top}/packaging/project.conf").read().decode()
            m = re.search(r"^APP_VERSION=(.+)$", conf, re.M)
            if not m or m.group(1).strip() != version:
                errs.append(f"source archive project.conf says APP_VERSION={m.group(1).strip() if m else None}, expected {version}")
        except KeyError:
            pass  # already reported as missing above
    return errs


def main(argv):
    args = list(argv)
    if not args or args[0].startswith("-"):
        print(__doc__)
        return 2
    folder = args.pop(0)
    version = None
    windows = False
    while args:
        a = args.pop(0)
        if a == "--with-windows":
            windows = True
        elif a == "--version" and args:
            version = args.pop(0)
        else:
            print(f"unknown option {a}", file=sys.stderr)
            return 2
    version = version or app_version()
    errs = check(folder, version, windows)
    for e in errs:
        print("MISMATCH " + e, file=sys.stderr)
    print(f"release set {folder}: version {version}: " + ("consistent" if not errs else f"{len(errs)} mismatch(es)"))
    return 1 if errs else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
