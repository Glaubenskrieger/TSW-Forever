#!/usr/bin/env python3
"""Fetch the PUBLISHED Flatpak part of the live GitHub Pages site (OSTree repository, install files, channels.json) into a folder, verified file by file against the
site-manifest.json that the release deployment published (scripts/assemble_pages_site.py). A later deployment of the web site alone is assembled from this copy, so that it
never removes, changes or rolls back published Flatpak data.

  preserve_live_site.py --site https://owner.github.io/Repo --out <new folder>
      writes <out>/repo/..., <out>/refs/<install files>, <out>/channels.json

Refuses: a missing or malformed manifest, a path outside repo/, channels.json and the install files, an absolute or ".." path, a file whose SHA-256 differs, and an
output folder that is not new or empty. Only https is accepted (http only for 127.0.0.1 in the tests). Exit 0 = every file fetched and verified.
"""
import argparse
import concurrent.futures
import hashlib
import json
import os
import re
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

ALLOWED = re.compile(r"^(repo/[A-Za-z0-9._/+=-]+|channels\.json|[A-Za-z0-9._-]+\.(flatpakref|flatpakrepo))$")


class PreserveError(Exception):
    pass


def _url_ok(url):
    p = urllib.parse.urlparse(url)
    if p.scheme == "https":
        return True
    return p.scheme == "http" and p.hostname in ("127.0.0.1", "localhost")


def _get(url, tries=4):
    last = None
    for i in range(tries):
        try:
            with urllib.request.urlopen(url, timeout=60) as r:  # noqa: S310 (scheme checked above)
                return r.read()
        except urllib.error.HTTPError as e:
            last = e
            if e.code in (403, 404):
                break
        except (urllib.error.URLError, TimeoutError, OSError) as e:
            last = e
        time.sleep(2 * (i + 1))
    raise PreserveError(f"cannot fetch {url}: {last}")


def _safe(rel):
    if not ALLOWED.match(rel) or rel.startswith("/") or ".." in rel.split("/") or "//" in rel:
        raise PreserveError(f"the manifest names a path that may not be preserved: {rel}")
    if "testing" in rel.lower():
        raise PreserveError(f"the public site carries no testing data: {rel}")


def preserve(site, out, workers=16):
    if not _url_ok(site):
        raise PreserveError("only https URLs are accepted")
    if os.path.exists(out) and (not os.path.isdir(out) or os.listdir(out)):
        raise PreserveError(f"the output folder must be new or empty: {out}")
    base = site.rstrip("/") + "/"
    try:
        doc = json.loads(_get(base + "site-manifest.json?nocache=" + str(int(time.time()))))
    except ValueError as e:
        raise PreserveError(f"site-manifest.json is not valid JSON: {e}")
    files = doc.get("files") if isinstance(doc, dict) else None
    if doc.get("schema") != 1 or not isinstance(files, dict) or not files:
        raise PreserveError("site-manifest.json is malformed or empty")
    for rel, digest in files.items():
        _safe(rel)
        if not isinstance(digest, str) or not re.fullmatch(r"[0-9a-f]{64}", digest):
            raise PreserveError(f"bad checksum for {rel}")
    if "repo/summary" not in files or "channels.json" not in files:
        raise PreserveError("the manifest lacks repo/summary or channels.json")

    def dest(rel):
        if rel.endswith((".flatpakref", ".flatpakrepo")):
            return os.path.join(out, "refs", rel)
        return os.path.join(out, rel)

    def one(item):
        rel, digest = item
        data = _get(base + urllib.parse.quote(rel, safe="/+=@"))
        if hashlib.sha256(data).hexdigest() != digest:
            raise PreserveError(f"{rel}: the live file does not match the manifest")
        d = dest(rel)
        os.makedirs(os.path.dirname(d), exist_ok=True)
        with open(d, "wb") as f:
            f.write(data)

    os.makedirs(out, exist_ok=True)
    with concurrent.futures.ThreadPoolExecutor(max_workers=workers) as ex:
        for _ in ex.map(one, sorted(files.items())):
            pass
    return len(files)


def main(argv):
    ap = argparse.ArgumentParser(description=__doc__.split("\n")[0])
    ap.add_argument("--site", required=True)
    ap.add_argument("--out", required=True)
    a = ap.parse_args(argv)
    try:
        n = preserve(a.site, a.out)
    except (PreserveError, OSError) as e:
        print(f"preserve_live_site: {e}", file=sys.stderr)
        return 1
    print(f"preserved {n} published files from {a.site}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
