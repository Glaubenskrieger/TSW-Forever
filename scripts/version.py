#!/usr/bin/env python3
"""Version tool of TSW-Forever (numeric MAJOR.MINOR.PATCH policy). Reads and writes only files of this workspace; no Git, no network.

  version.py show                 print APP_VERSION (the one line CI reads)
  version.py check                every place that carries the application version agrees; all crate versions are numeric
  version.py set <version> [--date YYYY-MM-DD]   change APP_VERSION in project.conf, both Cargo.toml/Cargo.lock entries and the metainfo release
  version.py final                exit 0 only if APP_VERSION has the required numeric form
  version.py valid <version>      exit 0 only for MAJOR.MINOR.PATCH
  version.py compare <a> <b>      print -1, 0 or 1 for two numeric versions
  version.py component <name>     print the version of an independent component: patch-engine | rdb

Three version axes that must never be mixed up: the APPLICATION version (launcher/GUI/Flatpak), the ENGINE version (crate tsw-patch) and the
CLIENT PATCH LEVEL (an integer in the signed manifest). The manifest FORMAT is a fourth, separate integer.
"""
import datetime
import os
import re
import sys

ROOT = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
CONF = "packaging/project.conf"
METAINFO = "packaging/flatpak/org.tswlauncher.Launcher.metainfo.xml"
APP_CRATES = {"launcher/Cargo.toml": "tsw-launch", "launcher-gui/Cargo.toml": "tsw-launcher-gui"}
# lock files that carry an entry for each crate (the GUI lock also lists the launcher and the engine as path dependencies)
LOCKS = {"launcher/Cargo.lock": ["tsw-launch"], "launcher-gui/Cargo.lock": ["tsw-launcher-gui", "tsw-launch"]}
ENGINE_TOML, ENGINE_LOCKS = "patcher/Cargo.toml", {"patcher/Cargo.lock": "tsw-patch", "launcher/Cargo.lock": "tsw-patch", "launcher-gui/Cargo.lock": "tsw-patch"}
RDB_TOML = "native/tsw-rdb/Cargo.toml"
MANIFEST_RS = "patcher/src/manifest.rs"

# the regular expression of semver.org, section "Is there a suggested regular expression"
SEMVER = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)(?:-((?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*)(?:\.(?:0|[1-9]\d*|\d*[a-zA-Z-][0-9a-zA-Z-]*))*))?(?:\+([0-9a-zA-Z-]+(?:\.[0-9a-zA-Z-]+)*))?\Z")


def parse(v):
    m = SEMVER.match(v) if isinstance(v, str) else None
    if not m:
        raise ValueError(f"not a SemVer 2.0.0 version: {v!r}")
    return int(m[1]), int(m[2]), int(m[3]), (m[4].split(".") if m[4] else []), m[5]


def compare(a, b):
    """SemVer 2.0.0 precedence (section 11); build metadata does not count."""
    (a1, a2, a3, ap, _), (b1, b2, b3, bp, _) = parse(a), parse(b)
    if (a1, a2, a3) != (b1, b2, b3):
        return -1 if (a1, a2, a3) < (b1, b2, b3) else 1
    if not ap or not bp:  # a version without pre-release is higher than one with
        return 0 if not ap and not bp else (1 if not ap else -1)
    for x, y in zip(ap, bp):
        if x == y:
            continue
        xn, yn = x.isdigit(), y.isdigit()
        if xn and yn:
            return -1 if int(x) < int(y) else 1
        if xn != yn:
            return -1 if xn else 1  # numeric identifiers rank below alphanumeric ones
        return -1 if x < y else 1
    return 0 if len(ap) == len(bp) else (-1 if len(ap) < len(bp) else 1)


def is_prerelease(v):
    return bool(parse(v)[3])


def _read(rel, root=ROOT):
    with open(os.path.join(root, rel), encoding="utf-8") as f:
        return f.read()


def _write(rel, text, root=ROOT):
    with open(os.path.join(root, rel), "w", encoding="utf-8") as f:
        f.write(text)


def conf_value(key, root=ROOT):
    for line in _read(CONF, root).splitlines():
        if line.startswith(key + "="):
            return line[len(key) + 1:].strip()
    raise KeyError(f"{key} missing in {CONF}")


def toml_version(rel, root=ROOT):
    m = re.search(r'^\[package\].*?^version\s*=\s*"([^"]+)"', _read(rel, root), re.S | re.M)
    if not m:
        raise ValueError(f"no package version in {rel}")
    return m[1]


def lock_version(rel, crate, root=ROOT):
    m = re.search(r'name = "%s"\nversion = "([^"]+)"' % re.escape(crate), _read(rel, root))
    return m[1] if m else None


def metainfo_release(root=ROOT):
    m = re.search(r'<release\s+version="([^"]+)"(?:[^>]*?type="([^"]+)")?', _read(METAINFO, root))
    return (m[1], m[2]) if m else (None, None)


def numeric_version(v):
    return re.fullmatch(r"(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)", v) is not None


def check(root=ROOT):
    """Return a list of problems (empty = consistent)."""
    errs = []
    try:
        app = conf_value("APP_VERSION", root)
        parse(app)
    except (KeyError, ValueError) as e:
        return [str(e)]
    if not numeric_version(app):
        errs.append(f"APP_VERSION {app}: use MAJOR.MINOR.PATCH without a suffix")
    for rel, crate in APP_CRATES.items():
        v = toml_version(rel, root)
        if v != app:
            errs.append(f"{rel}: {v} differs from APP_VERSION {app}")
    for rel, crates in LOCKS.items():
        for c in crates:
            v = lock_version(rel, c, root)
            if v != app:
                errs.append(f"{rel}: {c} is {v}, expected {app} (run cargo build once after `set`)")
    rel_v, rel_t = metainfo_release(root)
    if rel_v != app:
        errs.append(f"{METAINFO}: release {rel_v} differs from APP_VERSION {app}")
    want_t = "development" if is_prerelease(app) else "stable"
    if rel_t != want_t:
        errs.append(f"{METAINFO}: release type is {rel_t!r}, a {'pre-release' if want_t == 'development' else 'final version'} needs type=\"{want_t}\"")
    # independent components: valid SemVer, and the engine's crate version is the same in every lock file
    eng = toml_version(ENGINE_TOML, root)
    try:
        parse(eng)
        if not numeric_version(eng):
            errs.append(f"{ENGINE_TOML}: use MAJOR.MINOR.PATCH without a suffix")
    except ValueError as e:
        errs.append(f"{ENGINE_TOML}: {e}")
    for rel, crate in ENGINE_LOCKS.items():
        v = lock_version(rel, crate, root)
        if v != eng:
            errs.append(f"{rel}: {crate} is {v}, expected {eng}")
    if os.path.exists(os.path.join(root, RDB_TOML)):
        try:
            parse(toml_version(RDB_TOML, root))
        except ValueError as e:
            errs.append(f"{RDB_TOML}: {e}")
    # manifest format: an integer, the same in project.conf and in the header the engine writes - never a SemVer
    fmt = conf_value("PATCH_MANIFEST_FORMAT", root)
    if not re.fullmatch(r"[1-9]\d*", fmt):
        errs.append(f"PATCH_MANIFEST_FORMAT must be a positive integer, got {fmt!r}")
    elif f'pub const HEADER: &str = "tsw-forever-patch {fmt}";' not in _read(MANIFEST_RS, root):
        errs.append(f"{MANIFEST_RS}: the header does not carry format {fmt}")
    return errs


def set_version(new, date=None, root=ROOT):
    parse(new)
    if not numeric_version(new):
        raise ValueError(f"{new}: use MAJOR.MINOR.PATCH without a suffix")
    date = date or datetime.date.today().isoformat()
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        raise ValueError(f"bad date {date!r}")
    conf = _read(CONF, root)
    _write(CONF, re.sub(r"^APP_VERSION=.*$", f"APP_VERSION={new}", conf, count=1, flags=re.M), root)
    for rel in APP_CRATES:
        t = _read(rel, root)
        _write(rel, re.sub(r'(?s)(\[package\].*?^version\s*=\s*)"[^"]+"', lambda m: f'{m[1]}"{new}"', t, count=1, flags=re.M), root)
    for rel, crates in LOCKS.items():
        t = _read(rel, root)
        for c in crates:
            t = re.sub(r'(name = "%s"\nversion = )"[^"]+"' % re.escape(c), lambda m: f'{m[1]}"{new}"', t, count=1)
        _write(rel, t, root)
    kind = "development" if is_prerelease(new) else "stable"
    mi = _read(METAINFO, root)
    mi = re.sub(r'<release\s[^>]*?/>', f'<release version="{new}" date="{date}" type="{kind}" />', mi, count=1)
    _write(METAINFO, mi, root)


def main(argv):
    cmd = argv[1] if len(argv) > 1 else ""
    try:
        if cmd == "show":
            print(conf_value("APP_VERSION"))
        elif cmd == "check":
            errs = check()
            for e in errs:
                print("version check:", e, file=sys.stderr)
            if not errs:
                print(f"versions are consistent: application {conf_value('APP_VERSION')}, patch engine {toml_version(ENGINE_TOML)}, manifest format {conf_value('PATCH_MANIFEST_FORMAT')}")
            return 1 if errs else 0
        elif cmd == "set" and len(argv) >= 3:
            date = argv[argv.index("--date") + 1] if "--date" in argv else None
            set_version(argv[2], date)
            print(f"APP_VERSION set to {argv[2]}; run `cargo build` in launcher/ and launcher-gui/ if a lock file complains, then `version.py check`")
        elif cmd == "final":
            return 0 if numeric_version(conf_value("APP_VERSION")) else 1
        elif cmd == "valid" and len(argv) == 3:
            if not numeric_version(argv[2]):
                raise ValueError("use MAJOR.MINOR.PATCH without a suffix")
        elif cmd == "compare" and len(argv) == 4:
            if not numeric_version(argv[2]) or not numeric_version(argv[3]):
                raise ValueError("use MAJOR.MINOR.PATCH without a suffix")
            print(compare(argv[2], argv[3]))
        elif cmd == "component" and len(argv) == 3 and argv[2] in ("patch-engine", "rdb"):
            print(toml_version(ENGINE_TOML if argv[2] == "patch-engine" else RDB_TOML))
        else:
            print(__doc__)
            return 2
    except (ValueError, KeyError, OSError) as e:
        print(f"version.py: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
