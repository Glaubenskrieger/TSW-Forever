#!/usr/bin/env python3
"""Assemble the GitHub Pages site of the public repository: the web site AND the signed Flatpak (OSTree) repository, together, in one output folder.

GitHub Pages replaces the WHOLE site with every deployment. So the repository and the web site must never be deployed separately: each deployment is built here from
(a) the web site sources kept in the Git repository (optional `--website`), (b) the OSTree repository of this release (with the earlier commits carried over by the
release workflow), and (c) the install files. Each part has its own place and a name that the others may not use, so none can overwrite another:

  /repo/                              the OSTree repository (served as static files; Flatpak reads /repo/summary, /repo/refs/..., /repo/objects/...)
  /tsw-forever.flatpakrepo, /<app-id>.flatpakref   the install files
  /channels.json                      the publish guard's record (optional)
  /site-manifest.json                 SHA-256 of every file of repo/, the install files and channels.json: a later deployment of the web site alone re-fetches and verifies them
                                      (scripts/preserve_live_site.py), so that a web site update never removes or changes published Flatpak data
  /.nojekyll                          Pages must not run Jekyll over the repository files
  /index.html and anything else       the web site (only if `--website` is given); names above are reserved and refused in the web site

  assemble_pages_site.py --repo <ostree repo> --refs <folder with the install files> --out <new folder> [--website <folder>] [--channels-json <file>] [--app-id <id>]
  assemble_pages_site.py --website-only --website <folder> --out <new folder>     (no Flatpak repository has been published yet: the web site alone)

Refuses: a non-empty output folder, symbolic links anywhere, a repository without summary or without the stable ref, any ref or record that mentions testing (the public
repository carries stable only), a web site that uses a reserved name, and a result that exceeds the Pages size limit. Writes only below --out. Exit 0 = assembled.
"""
import argparse
import hashlib
import json
import os
import re
import shutil
import sys

APP_ID = "org.tswlauncher.Launcher"
LIMIT = 900 * 1024 * 1024  # GitHub Pages: published sites may be no larger than 1 GB; keep a margin
FILE_LIMIT = 90 * 1024 * 1024  # GitHub refuses files above 100 MB
RESERVED = {"repo", "channels.json", ".nojekyll", "site-manifest.json"}


class SiteError(Exception):
    pass


def _walk_files(root):
    for d, dirs, files in os.walk(root):
        for n in dirs + files:
            p = os.path.join(d, n)
            if os.path.islink(p):
                raise SiteError(f"symbolic link refused: {os.path.relpath(p, root)}")
        for n in files:
            yield os.path.join(d, n)


def _tree_size(root):
    total = 0
    for p in _walk_files(root):
        size = os.path.getsize(p)
        if size > FILE_LIMIT:
            raise SiteError(f"file too large for GitHub: {os.path.relpath(p, root)} ({size} bytes)")
        total += size
    return total


def check_repo(repo, app_id):
    if not os.path.isfile(os.path.join(repo, "summary")) or not os.path.isdir(os.path.join(repo, "objects")):
        raise SiteError("the OSTree repository has no summary or no objects")
    heads = os.path.join(repo, "refs", "heads")
    if not os.path.isdir(heads):
        raise SiteError("the OSTree repository has no refs/heads")
    refs = []
    for d, _, files in os.walk(heads):
        for n in files:
            refs.append(os.path.relpath(os.path.join(d, n), heads).replace(os.sep, "/"))
    for r in refs:
        if "testing" in r.lower():
            raise SiteError(f"the public repository must not carry a testing ref: {r}")
    want = f"app/{app_id}/x86_64/stable"
    if want not in refs:
        raise SiteError(f"the repository lacks the stable ref {want} (found: {sorted(refs)})")
    commit = open(os.path.join(heads, want)).read().strip()
    if not re.fullmatch(r"[0-9a-f]{64}", commit):
        raise SiteError(f"the stable ref does not hold a commit checksum")


def write_manifest(out):
    """site-manifest.json: SHA-256 of repo/**, the install files and channels.json (everything that is not the web site)."""
    files = {}
    for p in _walk_files(out):
        rel = os.path.relpath(p, out).replace(os.sep, "/")
        if rel.startswith("repo/") or rel == "channels.json" or rel.endswith((".flatpakref", ".flatpakrepo")):
            h = hashlib.sha256()
            with open(p, "rb") as f:
                for block in iter(lambda: f.read(1 << 20), b""):
                    h.update(block)
            files[rel] = h.hexdigest()
    with open(os.path.join(out, "site-manifest.json"), "w", encoding="utf-8") as f:
        json.dump({"schema": 1, "files": files}, f, indent=0, sort_keys=True)


def assemble_website_only(out, website):
    if website is None or not os.path.isdir(website):
        raise SiteError("--website-only needs the web site folder")
    if os.path.exists(out) and (not os.path.isdir(out) or os.listdir(out)):
        raise SiteError(f"the output folder must be new or empty: {out}")
    _tree_size(website)
    for n in os.listdir(website):
        if n in RESERVED or n.endswith((".flatpakrepo", ".flatpakref")):
            raise SiteError(f"the web site uses a name that belongs to the Flatpak repository: {n}")
    if not os.path.isfile(os.path.join(website, "index.html")):
        raise SiteError("the web site has no index.html")
    os.makedirs(out, exist_ok=True)
    shutil.copytree(website, out, dirs_exist_ok=True, symlinks=False)
    open(os.path.join(out, ".nojekyll"), "w").close()
    return _tree_size(out)


def assemble(repo, refs, out, website=None, channels_json=None, app_id=APP_ID):
    for label, p in (("repository", repo), ("install files", refs)):
        if not os.path.isdir(p):
            raise SiteError(f"{label} folder missing: {p}")
    if os.path.exists(out) and (not os.path.isdir(out) or os.listdir(out)):
        raise SiteError(f"the output folder must be new or empty: {out}")
    check_repo(repo, app_id)
    install_files = sorted(os.listdir(refs))
    if not any(n.endswith(".flatpakrepo") for n in install_files) or f"{app_id}.flatpakref" not in install_files:
        raise SiteError("the install files (.flatpakrepo and the stable .flatpakref) are missing")
    if any("testing" in n.lower() for n in install_files):
        raise SiteError("the public site must not carry testing install files")
    reserved = RESERVED | set(install_files)
    if website is not None:
        if not os.path.isdir(website):
            raise SiteError(f"web site folder missing: {website}")
        _tree_size(website)  # refuses links and oversized files
        for n in os.listdir(website):
            if n in reserved or n.endswith((".flatpakrepo", ".flatpakref")):
                raise SiteError(f"the web site uses a name that belongs to the Flatpak repository: {n}")
    if channels_json is not None and "testing" in open(channels_json).read().lower():
        raise SiteError("channels.json mentions testing")
    _tree_size(repo)
    os.makedirs(out, exist_ok=True)
    if website is not None:
        shutil.copytree(website, out, dirs_exist_ok=True, symlinks=False)
    shutil.copytree(repo, os.path.join(out, "repo"), symlinks=False)
    for n in install_files:
        shutil.copy2(os.path.join(refs, n), os.path.join(out, n))
    if channels_json is not None:
        shutil.copy2(channels_json, os.path.join(out, "channels.json"))
    open(os.path.join(out, ".nojekyll"), "w").close()
    if not os.path.exists(os.path.join(out, "index.html")):
        with open(os.path.join(out, "index.html"), "w") as f:
            f.write("<!doctype html><meta charset=utf-8><title>TSW-Forever Flatpak repository</title>"
                    "<p>Install: <code>flatpak install --user tsw-forever.flatpakrepo</code> or open "
                    f"<code>{app_id}.flatpakref</code> with your software center.</p>\n")
    total = _tree_size(out)
    if total > LIMIT:
        raise SiteError(f"the site is {total} bytes; GitHub Pages allows 1 GB")
    check_repo(os.path.join(out, "repo"), app_id)
    write_manifest(out)
    return total


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--repo")
    ap.add_argument("--refs")
    ap.add_argument("--website-only", action="store_true")
    ap.add_argument("--out", required=True)
    ap.add_argument("--website")
    ap.add_argument("--channels-json")
    ap.add_argument("--app-id", default=APP_ID)
    a = ap.parse_args(argv)
    try:
        if a.website_only:
            if a.repo or a.refs or a.channels_json:
                raise SiteError("--website-only takes no repository, install files or channels.json")
            total = assemble_website_only(a.out, a.website)
        else:
            if not a.repo or not a.refs:
                raise SiteError("--repo and --refs are required (or --website-only)")
            total = assemble(a.repo, a.refs, a.out, a.website, a.channels_json, a.app_id)
    except (SiteError, OSError) as e:
        print(f"assemble_pages_site: {e}", file=sys.stderr)
        return 1
    print(f"site assembled in {a.out} ({total} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
