#!/usr/bin/env python3
"""Read-only check before sharing or committing: does any text file of the project still look like it carries credentials?

Prints PATHS and LINE NUMBERS only, never the lines.
Exit 0 = nothing found, 1 = findings, 2 = the check could not run (a Git problem or a wrong call; NOTHING was checked then).

Usage: check_secrets_in_tree.py [--tree] [root]    (default root: the project root)

Two scopes:
* Default inside a Git repository (Git mode): exactly the files a commit could contain, `git ls-files --cached --others --exclude-standard` = every
  tracked file and every new file that is not ignored (also new files that were never `git add`ed). Of a tracked file with a STAGED change, the staged
  version is scanned as well as the working copy, so a value that was staged and then removed from the working copy is still found. Files that
  .gitignore keeps private (build output, local reports, private analysis areas) and the Git HISTORY are NOT scanned, because they never enter a commit;
  `prepublish_check.py --git --history` covers history. If Git fails (missing, damaged repository, unsafe ownership, an unreadable listed file) the check
  stops with exit 2 instead of silently checking fewer files.
* `--tree`, and the default outside a Git repository: the whole folder with os.walk, no Git. It skips only what is listed in SKIP_DIRS / SKIP_PREFIXES /
  SKIP_NAMES / SKIP_FILES below (private analysis areas, raw logs) and binary files; it does read ignored build folders, so it can be slow and can report
  vendored or runtime files that never leave the machine.
Both scopes use the same shapes, the same skip lists and the same REVIEWED_LINES.
"""
import hashlib
import os
import re
import stat
import subprocess
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


MAX_BYTES = 20_000_000
_GIT_ENV_DROP = ("GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_ALTERNATE_OBJECT_DIRECTORIES", "GIT_NAMESPACE", "GIT_COMMON_DIR")


class GitError(Exception):
    """Git could not tell which files to scan; the caller stops (exit 2) instead of scanning fewer files."""


def _findings_in(rel, data):
    """Line numbers of `data` (the bytes of the file `rel`) that match a shape and are not a reviewed line."""
    if b"\x00" in data[:4096] or not PREFILTER.search(data):
        return []
    reviewed = REVIEWED_LINES.get(rel, ())
    return [i for i, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1) if any(s.search(line) for s in SHAPES) and line_id(line) not in reviewed]


def _is_skipped(rel):
    """The same skip rules as the folder walk: a skipped directory on the way (by name or by prefix), or a skipped file name."""
    parts = rel.split("/")
    if parts[-1] in SKIP_FILES or SKIP_NAMES.match(parts[-1]):
        return True
    return any(parts[k - 1] in SKIP_DIRS or "/".join(parts[:k]).startswith(SKIP_PREFIXES) for k in range(1, len(parts)))


def scan(root):
    """The whole folder (--tree and outside Git): [(relative path, line number)]."""
    findings = []
    for d, dirs, files in os.walk(root):
        dirs[:] = [x for x in dirs if x not in SKIP_DIRS and not os.path.relpath(os.path.join(d, x), root).replace(os.sep, "/").startswith(SKIP_PREFIXES)]  # "/" on every platform
        for f in files:
            if f in SKIP_FILES or SKIP_NAMES.match(f):
                continue
            p = os.path.join(d, f)
            try:
                st = os.lstat(p)
                # only regular files: opening a named pipe (the old test reports contain some) would block forever
                if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_BYTES:
                    continue
                data = open(p, "rb").read()
            except OSError:
                continue
            rel = os.path.relpath(p, root).replace(os.sep, "/")
            findings.extend((rel, i) for i in _findings_in(rel, data))
    return findings


def _git(root, *args):
    env = {k: v for k, v in os.environ.items() if k not in _GIT_ENV_DROP}
    try:
        p = subprocess.run(["git", "-C", root, *args], capture_output=True, env=env, timeout=300)
    except (OSError, subprocess.SubprocessError) as e:
        raise GitError(f"git could not be run ({type(e).__name__})") from e
    if p.returncode != 0:
        first = (p.stderr.decode("utf-8", "replace").strip().splitlines() or ["no message"])[0][:160]
        raise GitError(f"git {args[0]} failed (exit {p.returncode}): {first}")
    return p.stdout


def in_git_repository(root):
    """Is there a `.git` entry in `root` or above it? Decided from the file system, so a missing or broken Git cannot pass for 'no repository'."""
    d = os.path.realpath(root)
    while True:
        if os.path.lexists(os.path.join(d, ".git")):
            return True
        parent = os.path.dirname(d)
        if parent == d:
            return False
        d = parent


def git_listed_files(root):
    """Every tracked file and every new, not ignored file below `root`, as paths relative to `root`."""
    if _git(root, "rev-parse", "--is-inside-work-tree").strip() != b"true":
        raise GitError("git does not see a work tree here")
    files = sorted({f.decode("utf-8", "surrogateescape") for f in _git(root, "ls-files", "-z", "--cached", "--others", "--exclude-standard").split(b"\0") if f})
    if not files:
        raise GitError("git listed no files")
    return files


def git_staged_files(root):
    """Paths with a staged change (added, copied, modified, renamed), relative to `root`."""
    out = _git(root, "diff", "--cached", "--name-only", "--relative", "--diff-filter=ACMR", "-z")
    return sorted({f.decode("utf-8", "surrogateescape") for f in out.split(b"\0") if f})


def scan_git(root):
    """Git mode: ([(path, line, staged)], number of files looked at). `staged` marks a line found only in the staged version of the file."""
    findings, looked, seen = [], 0, set()
    for rel in git_listed_files(root):
        if _is_skipped(rel):
            continue
        path = os.path.join(root, rel)
        try:
            st = os.lstat(path)
        except FileNotFoundError:
            continue  # tracked, deleted in the working copy: nothing there to scan
        except OSError as e:
            raise GitError(f"cannot read {rel} ({type(e).__name__})") from e
        if not stat.S_ISREG(st.st_mode) or st.st_size > MAX_BYTES:
            continue  # a link, a pipe, a submodule folder or a huge file: skipped like in the folder walk
        try:
            data = open(path, "rb").read()
        except OSError as e:
            raise GitError(f"cannot read {rel} ({type(e).__name__})") from e
        looked += 1
        for i in _findings_in(rel, data):
            findings.append((rel, i, False))
            seen.add((rel, i))
    for rel in git_staged_files(root):
        if _is_skipped(rel):
            continue
        data = _git(root, "cat-file", "blob", ":./" + rel)
        if len(data) > MAX_BYTES:
            continue
        looked += 1
        findings.extend((rel, i, True) for i in _findings_in(rel, data) if (rel, i) not in seen)
    return findings, looked


USAGE = "Usage: check_secrets_in_tree.py [--tree] [root]"


def main(argv):
    tree = "--tree" in argv
    rest = [a for a in argv if a != "--tree"]
    if any(a.startswith("-") for a in rest) or len(rest) > 1:
        print(USAGE, file=sys.stderr)
        return 2
    root = os.path.realpath(rest[0]) if rest else os.path.realpath(os.path.join(os.path.dirname(os.path.realpath(__file__)), ".."))
    if not os.path.isdir(root):
        print(f"not a folder: {root}", file=sys.stderr)
        return 2
    if not tree and in_git_repository(root):
        try:
            found, looked = scan_git(root)
        except GitError as e:
            print(f"check_secrets_in_tree: cannot determine the files to scan: {e}. NOTHING was checked. Fix Git, or use --tree to scan the whole folder.", file=sys.stderr)
            return 2
        for path, line, staged in found:
            print(f"{path}:{line}" + (" (staged version)" if staged else ""))
        print(f"{len(found)} suspicious line(s) in the {looked} Git-listed text file(s) (tracked and new, not ignored; ignored files and Git history are NOT scanned)" + (" - review these paths (values are not printed)" if found else ""))
        return 1 if found else 0
    found = scan(root)
    for path, line in found:
        print(f"{path}:{line}")
    print(f"{len(found)} suspicious line(s) in text files outside the private areas" + (" - review these paths (values are not printed)" if found else ""))
    return 1 if found else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
