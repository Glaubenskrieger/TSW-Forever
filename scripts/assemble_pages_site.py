#!/usr/bin/env python3
"""Assemble the GitHub Pages site of the public repository: the STATIC WEB SITE and nothing else.

Since 0.4.1 GitHub Pages carries the web site only. The installation files (Flatpak bundle, Windows installer, source archive) are assets of the GitHub release; the page links to
them at runtime. No Flatpak/OSTree repository, no install file for a Flatpak remote and no release file is ever put on Pages, so the Pages artifact is small and independent of any release.

  assemble_pages_site.py --website <folder> --out <new folder>
  assemble_pages_site.py --verify <assembled folder>

Copies the web site into the new folder, adds `.nojekyll` and checks the result. `--verify` runs the SAME checks on an already assembled site (what the production deployment does with the
artifact that the web site build kept, so the deployed bytes are checked by the same code that assembled them) and needs the marker `.nojekyll` in it. Refuses: a missing web site or one without index.html, a non-empty output folder, symbolic links,
any installation or repository file inside the web site (see FORBIDDEN_*), a file that is not plain web content, and a result above SIZE_LIMIT. Writes only below --out. Exit 0 = assembled.
"""
import argparse
import os
import shutil
import sys

SIZE_LIMIT = 8 * 1024 * 1024  # the web site is a few hundred kilobytes; a megabyte-sized surprise is refused
FILE_LIMIT = 2 * 1024 * 1024
# Names and folders that belong to a Flatpak repository, a Flatpak remote or a release: never on Pages
FORBIDDEN_DIRS = {"repo", "objects", "refs", "deltas", "summaries", "delta-indexes", "state", "tmp"}
FORBIDDEN_NAMES = {"summary", "summary.sig", "summary.idx", "summary.idx.sig", "config", "channels.json", "site-manifest.json", "SHA256SUMS"}
FORBIDDEN_SUFFIXES = (".flatpak", ".flatpakref", ".flatpakrepo", ".exe", ".msi", ".gz", ".tar", ".zip", ".gpg", ".asc", ".sig", ".filez", ".dirtree", ".dirmeta", ".commit", ".commitmeta", ".key", ".pem")
ALLOWED_SUFFIXES = (".html", ".css", ".js", ".svg", ".png", ".jpg", ".jpeg", ".webp", ".ico", ".json", ".txt", ".woff2", ".webmanifest", ".xml")


class SiteError(Exception):
    pass


def _walk(root):
    for d, dirs, files in os.walk(root):
        for n in dirs + files:
            p = os.path.join(d, n)
            if os.path.islink(p):
                raise SiteError(f"symbolic link refused: {os.path.relpath(p, root)}")
        yield d, dirs, files


def check_website(website, assembled=False):
    """Refuse anything that is not plain static web content; returns the total size. `assembled`: the folder is the result of assemble() and must carry the empty marker `.nojekyll` (and only that dot file)."""
    if website is None or not os.path.isdir(website):
        raise SiteError("the web site folder is missing")
    if not os.path.isfile(os.path.join(website, "index.html")):
        raise SiteError("the web site folder needs an index.html")
    total = 0
    for d, dirs, files in _walk(website):
        for n in dirs:
            if n in FORBIDDEN_DIRS or n.startswith("."):
                raise SiteError(f"a folder that belongs to a repository or a release is not part of the web site: {os.path.relpath(os.path.join(d, n), website)}")
        for n in files:
            rel = os.path.relpath(os.path.join(d, n), website)
            low = n.lower()
            if assembled and rel == ".nojekyll":
                if os.path.getsize(os.path.join(d, n)) != 0:
                    raise SiteError(".nojekyll must be empty")
                continue
            if n in FORBIDDEN_NAMES or low.endswith(FORBIDDEN_SUFFIXES) or n.startswith("."):
                raise SiteError(f"an installation or repository file is not part of the web site: {rel}")
            if not low.endswith(ALLOWED_SUFFIXES):
                raise SiteError(f"not a plain web file: {rel}")
            size = os.path.getsize(os.path.join(d, n))
            if size > FILE_LIMIT:
                raise SiteError(f"file too large for a web page: {rel} ({size} bytes)")
            total += size
    if total > SIZE_LIMIT:
        raise SiteError(f"the web site is {total} bytes: more than {SIZE_LIMIT}")
    if assembled and not os.path.isfile(os.path.join(website, ".nojekyll")):
        raise SiteError("an assembled site carries the marker .nojekyll")
    return total


def assemble(website, out):
    total = check_website(website)
    if os.path.exists(out) and os.listdir(out):
        raise SiteError(f"output folder is not empty: {out}")
    shutil.copytree(website, out, dirs_exist_ok=True, symlinks=False)
    with open(os.path.join(out, ".nojekyll"), "w", encoding="utf-8") as f:
        f.write("")
    return total


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--website")
    ap.add_argument("--out")
    ap.add_argument("--verify", help="check an already assembled site with the same rules (no output)")
    a = ap.parse_args(argv)
    if a.verify:
        if a.website or a.out:
            ap.error("--verify stands alone")
        try:
            total = check_website(a.verify, assembled=True)
        except (SiteError, OSError) as e:
            print(f"assemble_pages_site: {e}", file=sys.stderr)
            return 1
        print(f"site verified: {a.verify} ({total} bytes, web site only)")
        return 0
    if not a.website or not a.out:
        ap.error("--website and --out are needed (or --verify)")
    try:
        total = assemble(a.website, a.out)
    except (SiteError, OSError) as e:
        print(f"assemble_pages_site: {e}", file=sys.stderr)
        return 1
    count = sum(len(files) for _, _, files in os.walk(a.out))
    print(f"site assembled in {a.out}: {count} files, {total} bytes (web site only)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
