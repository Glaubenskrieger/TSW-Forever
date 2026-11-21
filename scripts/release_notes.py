#!/usr/bin/env python3
"""Extracts the release notes of a version from CHANGELOG.md (the entry `## [<version>] ...`) into a file. Shared by the release workflow (job `draft`) and the read-only release rehearsal.

  release_notes.py <tag or version> [--changelog CHANGELOG.md] [--out release-notes.md]

Exit 0: written. Exit 1: the changelog has no entry (or an empty one) for the version."""
import argparse
import pathlib
import re
import sys


def extract(text, version):
    pattern = re.compile(r"^## \[" + re.escape(version) + r"\][^\n]*\n(.*?)(?=^## \[|\Z)", re.M | re.S)
    match = pattern.search(text)
    if not match or not match.group(1).strip():
        return None
    return f"# TSW-Forever {version}\n\n" + match.group(1).strip() + "\n"


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("tag")
    ap.add_argument("--changelog", default="CHANGELOG.md")
    ap.add_argument("--out", default="release-notes.md")
    a = ap.parse_args(argv)
    version = a.tag[1:] if a.tag.startswith("v") else a.tag
    notes = extract(pathlib.Path(a.changelog).read_text(encoding="utf-8"), version)
    if notes is None:
        print(f"release_notes: no changelog entry for {version}", file=sys.stderr)
        return 1
    pathlib.Path(a.out).write_text(notes, encoding="utf-8")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
