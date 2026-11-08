#!/usr/bin/env python3
"""Audit of release artifacts: NOTHING that belongs to Microsoft or Funcom, no Windows program, no game data, no local build leftovers may be inside.

  check_release_artifacts.py <path> [<path> ...]     a folder (for example the installed files of the Flatpak) or a .tar.gz (the source archive)

For every file inside it checks (names and bytes only, nothing is run or extracted to disk):
  - the name: Windows programs/libraries/cabinets (.exe .dll .cab .msi .sys .ocx), Direct3D/D3DX libraries by name, game archives (.rdb .rdbdata .bxml .pak .swf),
    names of the original game and its patcher, Wine prefix files (drive_c, system.reg ...), key and credential file names
  - the first bytes: a Windows executable (`MZ`) anywhere is a violation (an ELF program of ours is fine)
  - the size: nothing over 64 MiB
Exit code 0 = clean, 1 = a violation (each is printed with the path inside the artifact), 2 = usage/environment error. Also usable as a module (`scan`).
"""
import os
import re
import sys
import tarfile

FORBIDDEN_NAME = re.compile(
    r"(\.(exe|dll|cab|msi|sys|ocx|rdb|rdbdata|bxml|pak|swf|dxvk-cache|spirv)$)|d3dcompiler|d3dx(9|10|11)|dxvk|vkd3d|TheSecretWorld(?!-)|ClientPatcher|Funcom"
    r"|(^|/)(drive_c|dosdevices|compatibilitytools\.d)(/|$)|(^|/)(system|user|userdef)\.reg$|\.(key|pem|p12|pfx)$|(^|/)id_(rsa|ed25519)|secring|\.gnupg",
    re.I,
)
MAX_SIZE = 64 * 1024 * 1024


def _entries_tar(fileobj, mode):
    with tarfile.open(fileobj=fileobj, mode=mode) as t:
        for m in t:
            if m.isfile():
                f = t.extractfile(m)
                yield m.name, m.size, f.read(2) if f else b""
            elif m.issym() or m.islnk():
                yield m.name + " (link)", 0, b""


def entries(path):
    """(name, size, first two bytes) of every file inside `path`."""
    if os.path.isdir(path):
        for d, _, fs in os.walk(path):
            for f in fs:
                p = os.path.join(d, f)
                if os.path.islink(p):
                    yield os.path.relpath(p, path) + " (link)", 0, b""
                    continue
                with open(p, "rb") as fh:
                    yield os.path.relpath(p, path), os.path.getsize(p), fh.read(2)
    elif path.endswith(".tar.gz") or path.endswith(".tgz"):
        yield from _entries_tar(open(path, "rb"), "r:gz")
    else:
        raise ValueError(f"unsupported artifact type: {path} (only a folder or a .tar.gz is audited: those are the things that are published besides the Flatpak bundle)")


def scan(path):
    """The violations (list of strings) of one artifact."""
    bad = []
    for name, size, head in entries(path):
        base = name.split(" (link)")[0]
        if FORBIDDEN_NAME.search(base):
            bad.append(f"{os.path.basename(path)}: forbidden name {name}")
        if head == b"MZ":
            bad.append(f"{os.path.basename(path)}: Windows executable (MZ header) {name}")
        if size > MAX_SIZE:
            bad.append(f"{os.path.basename(path)}: {name} is {size} bytes (limit {MAX_SIZE})")
    return bad


def main(argv):
    if not argv:
        print(__doc__)
        return 2
    total, bad = 0, []
    for p in argv:
        try:
            n = sum(1 for _ in entries(p))
            found = scan(p)
        except (OSError, ValueError, tarfile.TarError) as e:
            print(f"check_release_artifacts: {p}: {e}", file=sys.stderr)
            return 2
        total += n
        bad += found
        print(f"{os.path.basename(p.rstrip('/'))}: {n} files, {len(found)} violation(s)")
    for b in bad:
        print("VIOLATION " + b, file=sys.stderr)
    print("clean: no Microsoft/Funcom file, no Windows program, no game data" if not bad else f"{len(bad)} violation(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
