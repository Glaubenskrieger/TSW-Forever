#!/usr/bin/env python3
"""Write the UNSIGNED launcher update metadata (`update-stable.txt`) for a release, in the format `launcher/src/update` parses and checks. The metadata is then signed
OFFLINE with the owner's minisign key (`minisign -S -s <key> -m update-stable.txt`, giving `update-stable.txt.minisig`) and both files are attached to the release:
the launcher fetches `<repo>/releases/latest/download/update-stable.txt(.minisig)`. Nothing here signs, uploads or publishes.

  make_update_metadata.py --version 0.3.5 --installer <setup.exe> [--previous-version 0.3.4 --previous-installer <setup.exe>] [--valid-days 180]
                          [--repo-url https://github.com/<owner>/<repo>] [--out update-stable.txt] [--issued 2026-12-01T10:00:00Z] [--flatpak-ref ...]

The installer files are read to take their SIZE and SHA-256; their names must be tsw-forever-<version>-windows-setup.exe. The validity window is at most 400 days (the
launcher refuses longer ones). Re-run for every release; the launcher refuses expired metadata.
"""
import argparse
import datetime
import hashlib
import os
import re
import sys

ROOT = os.path.realpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
VERSION = re.compile(r"^(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})\.(0|[1-9]\d{0,5})$")
MAX_DAYS = 400


def conf(key):
    for line in open(os.path.join(ROOT, "packaging", "project.conf"), encoding="utf-8"):
        if line.startswith(key + "="):
            return line.split("=", 1)[1].strip()
    raise SystemExit(f"{key} missing in packaging/project.conf")


def file_info(path, version):
    name = f"tsw-forever-{version}-windows-setup.exe"
    if os.path.basename(path) != name:
        raise ValueError(f"the installer of {version} must be called {name}, not {os.path.basename(path)}")
    h, n = hashlib.sha256(), 0
    with open(path, "rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
            n += len(chunk)
    if n == 0:
        raise ValueError(f"{path} is empty")
    with open(path, "rb") as f:
        if f.read(2) != b"MZ":
            raise ValueError(f"{path} is not a Windows executable")
    return name, n, h.hexdigest()


def parse_time(text):
    return datetime.datetime.strptime(text, "%Y-%m-%dT%H:%M:%SZ").replace(tzinfo=datetime.timezone.utc)


def build(version, installer, previous_version=None, previous_installer=None, valid_days=180, repo_url=None, issued=None, flatpak_ref=None):
    if not VERSION.match(version):
        raise ValueError(f"{version!r} is not a plain MAJOR.MINOR.PATCH version")
    if not 1 <= valid_days <= MAX_DAYS:
        raise ValueError(f"the validity must be 1..{MAX_DAYS} days")
    repo_url = (repo_url or conf("REPO_URL")).rstrip("/")
    if not repo_url.startswith("https://"):
        raise ValueError("the repository address must be https")
    start = parse_time(issued) if issued else datetime.datetime.now(datetime.timezone.utc).replace(microsecond=0)
    end = start + datetime.timedelta(days=valid_days)
    fmt = "%Y-%m-%dT%H:%M:%SZ"
    name, size, sha = file_info(installer, version)
    base = f"{repo_url}/releases/download"
    lines = ["tsw-forever-update 1", "channel: stable", f"version: {version}", f"issued: {start.strftime(fmt)}", f"expires: {end.strftime(fmt)}",
             f"windows-installer: {name}", f"windows-url: {base}/v{version}/{name}", f"windows-size: {size}", f"windows-sha256: {sha}"]
    if (previous_version is None) != (previous_installer is None):
        raise ValueError("give both --previous-version and --previous-installer, or neither")
    if previous_version:
        if not VERSION.match(previous_version):
            raise ValueError(f"{previous_version!r} is not a plain version")
        tup = lambda v: tuple(int(x) for x in v.split("."))
        if tup(previous_version) >= tup(version):
            raise ValueError("the previous version must be older than the version")
        pname, psize, psha = file_info(previous_installer, previous_version)
        lines += [f"previous-version: {previous_version}", f"previous-installer: {pname}", f"previous-url: {base}/v{previous_version}/{pname}", f"previous-size: {psize}", f"previous-sha256: {psha}"]
    lines.append(f"notes-url: {repo_url}/releases/tag/v{version}")
    if flatpak_ref:
        if not re.fullmatch(r"app/[A-Za-z0-9_.]+/[a-z0-9_]+/stable", flatpak_ref):
            raise ValueError("flatpak-ref must be a stable ref")
        lines.append(f"flatpak-ref: {flatpak_ref}")
    text = "\n".join(lines) + "\n"
    if len(text) > 16 * 1024:
        raise ValueError("the metadata would be too large")
    return text


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--version", required=True)
    ap.add_argument("--installer", required=True)
    ap.add_argument("--previous-version")
    ap.add_argument("--previous-installer")
    ap.add_argument("--valid-days", type=int, default=180)
    ap.add_argument("--repo-url")
    ap.add_argument("--issued")
    ap.add_argument("--flatpak-ref")
    ap.add_argument("--out", default="update-stable.txt")
    a = ap.parse_args(argv)
    try:
        text = build(a.version, a.installer, a.previous_version, a.previous_installer, a.valid_days, a.repo_url, a.issued, a.flatpak_ref)
    except (ValueError, OSError) as e:
        print(f"make_update_metadata: {e}", file=sys.stderr)
        return 1
    with open(a.out, "w", encoding="ascii", newline="\n") as f:
        f.write(text)
    print(f"wrote {a.out}; now sign it OFFLINE: minisign -S -s <your-key> -m {a.out}   (gives {a.out}.minisig)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
