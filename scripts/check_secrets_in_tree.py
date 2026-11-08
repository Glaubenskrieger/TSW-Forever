#!/usr/bin/env python3
"""Read-only check before sharing or committing: does any text file of the project tree still look like it carries credentials?

Prints PATHS and LINE NUMBERS only, never the lines. Skips what .gitignore keeps private (analysis/runtime, analysis/binaries, raw logs,
build output) because those never leave the machine, and binary files. It does not use Git. Exit 0 = nothing found, 1 = findings.
Usage: check_secrets_in_tree.py [root]    (default: the project root)
"""
import hashlib
import os
import re
import stat
import sys

sys.path.insert(0, os.path.dirname(os.path.realpath(__file__)))
import secretmask  # noqa: E402

SKIP_DIRS = {".git", "target", "__pycache__", "node_modules"}
SKIP_PREFIXES = ("analysis/runtime", "analysis/binaries", "tools/vendor")  # tools/vendor: third-party packages and their npm cache
SKIP_NAMES = re.compile(r"^(ClientLog.*\.txt|Prefs_3\.(xml|bxml)|last-launch\.txt|wine\.(log|stdout)|runner\.(stdout|stderr)|stdout\.masked\.log)$")
SKIP_FILES = {"test_secretmask.py", "test_mask_stream.py", "test_run_filtered.sh", "secretmask.py", "safe_log_view.py"}  # contain INVENTED test values on purpose
# Lines that match a shape below and were REVIEWED by a person: they carry invented test values, are the detection patterns themselves, or document the format
# with placeholders. Pinned one by one as (path -> SHA-256 prefix of the stripped line), never by file or folder: when a pinned line changes, or the same text
# appears in another file, it is reported again. A real value in any other line of these files is found as before. `line_id()` computes the pin.
REVIEWED_LINES = {
    "scripts/check_secrets_in_tree.py": {"3142bfc1a9fe1bc0", "5df674a9e3ef4fb8"},  # the detection patterns
    "scripts/test_check_secrets_in_tree.py": {"81891badf6b36c1f", "979bb076a56e2234"},  # fixtures with an invented value
    "scripts/test_mask_launch_config.py": {"3902d79577808a3e", "d03d56c154123e50"},
    "scripts/test_prepublish_check.py": {"69444a2c49770ee5"},
    "launcher/src/launch.rs": {"50ec32a5625980db"},
    "launcher/src/diaglog.rs": {"e6c97586e3aa7f6b", "9b614b0a769cb359"},
    "documentation/security.md": {"1f6ad2a6fad21990", "004608982b37174f"},  # describes the client's log line with placeholders
}

# Exact, individually reviewed synthetic masking fixtures, also checked in source-archive audits.
# No prefix/whole-file exception: changed lines must be reviewed again.

# Hash-pinned invented values in masking regression tests (no real credentials).
REVIEWED_LINES.setdefault('launcher/src/diaglog.rs', set()).update({'d96dee075750b4f7'})
REVIEWED_LINES.setdefault('launcher/src/secrets.rs', set()).update({'6e659bf511854d57', '4613b8f1a913bd3a', '4a9bb026d90fb9e5', 'e1e185dc6155521e', '3a673f4f5d9282d1'})


def line_id(line):
    """The pin of a line: first 16 hex digits of the SHA-256 of the stripped line."""
    return hashlib.sha256(line.strip().encode("utf-8")).hexdigest()[:16]


SHAPES = [
    re.compile(r"(?i)registering with nick"),
    re.compile(r"(?i)\b(?:pass(?:word|wd)?|pwd|secret|token|api[_-]?key)\b\s*[:=]\s*[\"']?(?!<|…|\.\.\.|\s)[A-Za-z0-9+/_\-]{6,}"),
    re.compile(r"(?i)://[^/\s:@<]+:[^/\s@<]+@"),
    re.compile(r"(?i)-(?:loginkey|statindex)\s+(?![<…\.\s])[A-Za-z0-9]{6,}"),
    re.compile(r"\beyJ[A-Za-z0-9_\-]{8,}\.[A-Za-z0-9_\-]{8,}\."),
]


# cheap byte-level pre-filter: only files that contain one of these words are looked at line by line (the tree has ~20000 files)
PREFILTER = re.compile(rb"(?i)registering with nick|pass(?:word|wd)?\b|pwd|secret|token|api[_-]?key|://[^/\s:@]+:|loginkey|statindex|eyJ")


def scan(root):
    findings = []
    for d, dirs, files in os.walk(root):
        rel = os.path.relpath(d, root)
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS and not os.path.relpath(os.path.join(d, x), root).replace(os.sep, "/").startswith(SKIP_PREFIXES)]  # "/" on every platform
        for f in files:
            if f in SKIP_FILES or SKIP_NAMES.match(f):
                continue
            p = os.path.join(d, f)
            try:
                st = os.lstat(p)
                # only regular files: opening a named pipe (the old test reports contain some) would block forever
                if not stat.S_ISREG(st.st_mode) or st.st_size > 20_000_000:
                    continue
                data = open(p, "rb").read()
            except OSError:
                continue
            if b"\x00" in data[:4096] or not PREFILTER.search(data):
                continue
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            reviewed = REVIEWED_LINES.get(rel, ())
            for i, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1):
                if any(s.search(line) for s in SHAPES) and line_id(line) not in reviewed:
                    findings.append((rel, i))
    return findings


def main(argv):
    root = os.path.realpath(argv[0]) if argv else os.path.realpath(os.path.join(os.path.dirname(os.path.realpath(__file__)), ".."))
    found = scan(root)
    for path, line in found:
        print(f"{path}:{line}")
    print(f"{len(found)} suspicious line(s) in text files outside the private areas" + (" - review these paths (values are not printed)" if found else ""))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
