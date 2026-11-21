#!/usr/bin/env python3
"""Release candidate manifest: ties the artifacts of ONE build (push to main) to ONE commit, so that the release tag can prove that it publishes exactly those files.

  release_candidate.py create <parts-dir> --version V --commit SHA --out candidate.json
  release_candidate.py verify <parts-dir> candidate.json --version V --commit SHA

<parts-dir> holds the downloaded artifacts, one folder per artifact, named tsw-forever-<V>-<kind>-<SHA> with kind in flatpak, source, windows:
  flatpak: tsw-forever-<V>.flatpak, SHA256SUMS-flatpak     source: tsw-forever-<V>-src.tar.gz, SHA256SUMS-source     windows: tsw-forever-<V>-windows-setup.exe, SHA256SUMS-windows
`create` records the SHA-256 of every file; `verify` refuses a different version or commit, a missing or additional file, and any byte that differs. Exit 0 = ok. Writes nothing
but --out; reads nothing but the given folders.
"""
import argparse
import hashlib
import json
import os
import re
import sys

SCHEMA = 1


def expected(version):
    return {
        "flatpak": [f"tsw-forever-{version}.flatpak", "SHA256SUMS-flatpak"],
        "source": [f"tsw-forever-{version}-src.tar.gz", "SHA256SUMS-source"],
        "windows": [f"tsw-forever-{version}-windows-setup.exe", "SHA256SUMS-windows"],
    }


class CandidateError(Exception):
    pass


def _sha(path):
    h = hashlib.sha256()
    with open(path, "rb") as f:
        for block in iter(lambda: f.read(1 << 20), b""):
            h.update(block)
    return h.hexdigest()


def _check_ids(version, commit):
    if not re.fullmatch(r"\d+\.\d+\.\d+", version):
        raise CandidateError(f"not a version: {version}")
    if not re.fullmatch(r"[0-9a-f]{40}", commit):
        raise CandidateError(f"not a full commit id: {commit}")


def scan(parts, version, commit):
    """{artifact folder: {file: sha256}} for exactly the expected files; refuses anything missing, additional or linked."""
    _check_ids(version, commit)
    want = expected(version)
    found = {}
    entries = sorted(os.listdir(parts))
    names = {f"tsw-forever-{version}-{kind}-{commit}": kind for kind in want}
    if sorted(entries) != sorted(names):
        raise CandidateError(f"the parts are exactly {sorted(names)} (found {entries})")
    for folder, kind in names.items():
        d = os.path.join(parts, folder)
        if os.path.islink(d) or not os.path.isdir(d):
            raise CandidateError(f"{folder} is not a plain folder")
        files = sorted(os.listdir(d))
        if files != sorted(want[kind]):
            raise CandidateError(f"{folder} holds exactly {sorted(want[kind])} (found {files})")
        found[folder] = {}
        for n in files:
            p = os.path.join(d, n)
            if os.path.islink(p) or not os.path.isfile(p) or os.path.getsize(p) == 0:
                raise CandidateError(f"{folder}/{n} is not a non-empty regular file")
            found[folder][n] = _sha(p)
    return found


def create(parts, version, commit, out):
    files = scan(parts, version, commit)
    doc = {"schema": SCHEMA, "version": version, "commit": commit, "artifacts": files}
    with open(out, "w", encoding="utf-8") as f:
        json.dump(doc, f, indent=2, sort_keys=True)
        f.write("\n")
    return doc


def verify(parts, manifest, version, commit):
    with open(manifest, encoding="utf-8") as f:
        doc = json.load(f)
    if doc.get("schema") != SCHEMA or doc.get("version") != version or doc.get("commit") != commit:
        raise CandidateError(f"the manifest belongs to {doc.get('version')} / {doc.get('commit')}, not to {version} / {commit}")
    now = scan(parts, version, commit)
    if now != doc.get("artifacts"):
        bad = sorted(k for k in set(now) | set(doc.get("artifacts") or {}) if now.get(k) != (doc.get("artifacts") or {}).get(k))
        raise CandidateError(f"the artifacts differ from the manifest: {bad}")
    return doc


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    sub = ap.add_subparsers(dest="cmd", required=True)
    c = sub.add_parser("create")
    c.add_argument("parts")
    c.add_argument("--version", required=True)
    c.add_argument("--commit", required=True)
    c.add_argument("--out", required=True)
    v = sub.add_parser("verify")
    v.add_argument("parts")
    v.add_argument("manifest")
    v.add_argument("--version", required=True)
    v.add_argument("--commit", required=True)
    a = ap.parse_args(argv)
    try:
        if a.cmd == "create":
            create(a.parts, a.version, a.commit, a.out)
            print(f"candidate manifest written: {a.out}")
        else:
            verify(a.parts, a.manifest, a.version, a.commit)
            print("candidate verified: every artifact is the one that was built for this commit")
    except (CandidateError, OSError, ValueError) as e:
        print(f"release_candidate: {e}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
