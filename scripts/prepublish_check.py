#!/usr/bin/env python3
"""Pre-publication check: is everything that Git would pick up fit for a PUBLIC repository?

  scripts/prepublish_check.py            filesystem mode: applies .gitignore itself (no Git needed), checks every file that would be committed
  scripts/prepublish_check.py --git      additionally asks Git (run it yourself; agents must not): tracked files and untracked-not-ignored files,
                                         and fails for files that are tracked although .gitignore now ignores them
  scripts/prepublish_check.py --history  with --git: also scans every commit of every branch for forbidden paths and secret shapes
  scripts/prepublish_check.py --list     print the candidate files (one per line) instead of the summary

What it checks (and ONLY this): top-level allow-list, forbidden folders/suffixes/names (proprietary binaries, game archives, Wine prefix files, shader caches,
packages, local build output, reports, logs), executable/archive magic bytes, file size (2 MiB) and total size, images only in the icon folders,
secret shapes (private keys, tokens, passwords in text, credentials in URLs, the project's own account-data shapes), the local user's home path
and optional extra markers (PREPUBLISH_MARKERS=a,b), symbolic links and special files. It does NOT judge copyright, licences or the content of
documents; see documentation/security.md. Values of findings are never printed, only paths and line numbers.
Exit code 0 = no violation, 1 = at least one violation, 2 = usage / environment error.
"""
import fnmatch
import getpass
import os
import re
import stat
import subprocess
import sys

HERE = os.path.dirname(os.path.realpath(__file__))
sys.path.insert(0, HERE)
import check_secrets_in_tree as cst  # noqa: E402  (its SHAPES and skip lists are the project's account-data shapes)

ROOT = os.path.realpath(os.path.join(HERE, ".."))
TOP_ALLOWED = {"assets", "rustfmt.toml", ".cargo", "THIRD-PARTY-NOTICES.md", ".github", ".gitignore", "AGENTS.md", "CHANGELOG.md", "README.md", "documentation", "launcher", "launcher-gui", "packaging", "patcher", "scripts"}
TOP_ALLOWED_PREFIXES = ("LICENSE", "COPYING", "NOTICE")
FORBIDDEN_SUFFIX = (".exe", ".dll", ".msi", ".cab", ".sys", ".rdb", ".bxml", ".swf", ".pak", ".dmp", ".so", ".dylib", ".elf", ".o", ".a", ".pdb", ".reg",
                    ".flatpak", ".ostree", ".appimage", ".deb", ".rpm", ".zip", ".7z", ".tar", ".gz", ".xz", ".zst", ".bz2", ".rar", ".iso", ".log", ".pyc", ".pem",
                    ".p12", ".pfx", ".key", ".gpg", ".sqlite", ".db", ".tmp", ".swp")
FORBIDDEN_DIRS = {"analysis", "reports", "logs", "t", "tasks", "native", "research", "build", "target", "vendor", "__pycache__", ".git", ".flatpak-builder", "drive_c",
                  "dosdevices", "compatibilitytools.d", ".agents", ".claude", ".codex", ".aws", ".var", ".cache", ".local", "node_modules"}
FORBIDDEN_NAME = re.compile(r"(?i)^(ClientLog.*\.txt|Prefs_3\.(xml|bxml)|last-launch\.txt|system\.reg|user\.reg|userdef\.reg|tsw-launch-prefix\.txt|shader\.cache.*|"
                            r"TheSecretWorld.*|ClientPatcher.*|\.env(\..*)?|id_rsa.*|id_ed25519.*|\.mcp\.json|.*\.local\.json)$")
MAGIC = [(b"MZ", "Windows executable/DLL"), (b"\x7fELF", "ELF binary"), (b"MSCF", "Microsoft cabinet"), (b"7z\xbc\xaf", "7-Zip archive"), (b"PK\x03\x04", "ZIP archive"),
         (b"\x1f\x8b", "gzip data"), (b"\xfd7zXZ", "xz data"), (b"(\xb5/\xfd", "zstd data"), (b"SQLite format 3", "SQLite database"), (b"WINE REGISTRY", "Wine registry"),
         (b"Rar!", "RAR archive"), (b"\xca\xfe\xba\xbe", "Mach-O/Java class")]
IMAGE_SUFFIX = (".png", ".jpg", ".jpeg", ".gif", ".webp", ".bmp", ".ico", ".svg", ".icns", ".tga", ".dds")
IMAGE_DIRS = ("launcher-gui/assets/", "launcher-gui/packaging/linux/icons/")
# Targeted exceptions (no blanket permission for a folder or a size): path -> (SHA-256 of the approved content, size limit in bytes). The exception holds only
# while the file is byte-identical to what was reviewed AND documentation/third-party-components.md exists; a changed file fails the check until reviewed again.
BRANDING_EXCEPTIONS = {
    "assets/branding/Dramatisches Fraktions-Emblem mit Ankh.png": ("c85adf050c5d78fa88af085f73c69583dcfadc208e0b70b4ca967bc1d46380c9", 2_500_000),
}
BRANDING_DOC = "documentation/third-party-components.md"
MAX_FILE = 2 * 1024 * 1024
MAX_TOTAL = 25 * 1024 * 1024
PRIVATE_KEY = re.compile(rb"-----BEGIN (?:RSA |EC |DSA |OPENSSH |PGP |ENCRYPTED )?PRIVATE KEY(?: BLOCK)?-----")
TOKENS = re.compile(rb"\b(?:ghp_[A-Za-z0-9]{30,}|github_pat_[A-Za-z0-9_]{30,}|gho_[A-Za-z0-9]{30,}|AKIA[0-9A-Z]{16}|xox[baprs]-[A-Za-z0-9-]{10,}|sk-[A-Za-z0-9]{32,}|AIza[0-9A-Za-z_-]{35})\b")
# Files that contain INVENTED credentials (test fixtures of the masking tools) or only DESCRIBE the shapes on purpose. They are named one by one (a new file is
# never exempt); for them the credential-shape and token checks are skipped, private-key blocks and the home-path check still apply.
KNOWN_FIXTURES = {
    "documentation/security.md", "launcher/src/diaglog.rs", "launcher/src/launch.rs", "launcher/src/secrets.rs",
    "scripts/check_secrets_in_tree.py", "scripts/prepublish_check.py", "scripts/test_prepublish_check.py", "scripts/test_check_secrets_in_tree.py",
    "scripts/test_fps_cache_cli.sh", "scripts/test_mask_launch_config.py", "scripts/test_secretmask.py", "scripts/test_mask_stream.py",
    "scripts/test_run_filtered.sh", "scripts/secretmask.py", "scripts/safe_log_view.py", "scripts/mask_stream.py", "scripts/mask_launch_config.py",
}


# ---- .gitignore -------------------------------------------------------------------------------------------------------------------------

def _conv(p):
    out, i = "", 0
    while i < len(p):
        c = p[i]
        if p.startswith("**/", i):
            out += "(?:.*/)?"; i += 3
        elif p.startswith("/**", i) and i + 3 == len(p):
            out += "/.*"; i += 3
        elif p.startswith("**", i):
            out += ".*"; i += 2
        elif c == "*":
            out += "[^/]*"; i += 1
        elif c == "?":
            out += "[^/]"; i += 1
        elif c == "[":
            j = p.find("]", i + 2)
            if j < 0:
                out += re.escape(c); i += 1
            else:
                body = p[i + 1:j]
                body = "^" + body[1:] if body[:1] == "!" else body
                out += "[" + body.replace("\\", "\\\\") + "]"; i = j + 1
        elif c == "\\" and i + 1 < len(p):
            out += re.escape(p[i + 1]); i += 2
        else:
            out += re.escape(c); i += 1
    return out


class Rule:
    def __init__(self, line):
        self.neg = line.startswith("!")
        p = line[1:] if self.neg else line
        self.dir_only = p.endswith("/")
        p = p.rstrip("/")
        anchored = "/" in p
        p = p.lstrip("/")
        self.rx = re.compile(("^" if anchored else "^(?:.*/)?") + _conv(p) + "$")

    def match(self, rel, is_dir):
        return (is_dir or not self.dir_only) and bool(self.rx.match(rel))


def load_rules(root):
    rules = []
    try:
        for line in open(os.path.join(root, ".gitignore"), encoding="utf-8"):
            line = line.rstrip("\n").rstrip()
            if line and not line.startswith("#"):
                rules.append(Rule(line))
    except OSError:
        pass
    return rules


def ignored(rules, rel, is_dir):
    """Last matching rule wins (Git semantics); parents are handled by the walk, which never enters an ignored directory."""
    res = False
    for r in rules:
        if r.match(rel, is_dir):
            res = not r.neg
    return res


def walk_candidates(root, rules):
    """-> (candidates, ignored_top) where ignored_top = {top-level entry: kind}; ignored directories are not entered."""
    cands, skipped = [], {}
    for d, dirs, files in os.walk(root):
        rel_d = "" if d == root else os.path.relpath(d, root).replace(os.sep, "/")  # always "/" (the .gitignore rules and the allow-lists use it; matters on Windows)
        keep = []
        for x in sorted(dirs):
            rel = f"{rel_d}/{x}" if rel_d else x
            if x == ".git" and not rel_d:
                continue
            if ignored(rules, rel, True):
                skipped.setdefault(rel, "dir")
            else:
                keep.append(x)
        dirs[:] = keep
        for f in sorted(files):
            rel = f"{rel_d}/{f}" if rel_d else f
            if ignored(rules, rel, False):
                skipped.setdefault(rel, "file")
            else:
                cands.append(rel)
        # symbolic links to directories appear in `dirs` but os.walk does not follow them: treat them as candidates
        for x in list(keep):
            if os.path.islink(os.path.join(d, x)):
                cands.append(f"{rel_d}/{x}" if rel_d else x)
                dirs.remove(x)
    return cands, skipped


# ---- checks ------------------------------------------------------------------------------------------------------------------------------

def markers():
    m = [f"/home/{getpass.getuser()}/"] if getpass.getuser() not in ("root", "") else []
    m += [x for x in os.environ.get("PREPUBLISH_MARKERS", "").split(",") if x]
    return [x.encode() for x in m]


def check_file(root, rel, mk):
    """-> list of (kind, message). `kind` is 'violation'."""
    out = []
    p = os.path.join(root, rel)
    parts = rel.split("/")
    base = parts[-1]
    if parts[0] not in TOP_ALLOWED and not parts[0].startswith(TOP_ALLOWED_PREFIXES):
        out.append(f"top-level entry {parts[0]!r} is not on the allow-list (decide first, then add it to TOP_ALLOWED in scripts/prepublish_check.py)")
    for c in parts[:-1]:
        if c in FORBIDDEN_DIRS:
            out.append(f"inside the forbidden folder {c!r}")
    if base.lower().endswith(FORBIDDEN_SUFFIX):
        out.append(f"forbidden file type {os.path.splitext(base)[1] or base!r}")
    if FORBIDDEN_NAME.match(base):
        out.append("forbidden file name (game, prefix, cache or local-configuration file)")
    try:
        st = os.lstat(p)
    except OSError as e:
        return [f"cannot stat: {e.strerror}"]
    if stat.S_ISLNK(st.st_mode):
        return out + ["symbolic link (resolve it or ignore it)"]
    if not stat.S_ISREG(st.st_mode):
        return out + ["not a regular file (pipe/socket/device)"]
    exc = BRANDING_EXCEPTIONS.get(rel)
    if exc:
        import hashlib
        if hashlib.sha256(open(p, "rb").read()).hexdigest() != exc[0]:
            out.append("approved branding file has changed since its review (update BRANDING_EXCEPTIONS and the provenance document after a new review)")
        elif not os.path.isfile(os.path.join(root, BRANDING_DOC)):
            out.append(f"approved branding file without its provenance document {BRANDING_DOC}")
        elif st.st_size > exc[1]:
            out.append(f"branding file is {st.st_size // 1024} KiB, above its exception limit")
        return out + [m for m in [] ]
    if st.st_size > MAX_FILE:
        out.append(f"file is {st.st_size // 1024} KiB, larger than the {MAX_FILE // 1024} KiB limit")
    if base.lower().endswith(IMAGE_SUFFIX) and not rel.startswith(IMAGE_DIRS):
        out.append("image outside the icon folders (provenance and rights must be recorded first)")
    try:
        data = open(p, "rb").read(MAX_FILE + 1) if st.st_size <= MAX_FILE else open(p, "rb").read(4096)
    except OSError as e:
        return out + [f"cannot read: {e.strerror}"]
    for sig, what in MAGIC:
        if data.startswith(sig):
            out.append(f"binary content: {what}")
            break
    if b"\x00" in data[:4096]:
        return out  # binary: no text checks
    if PRIVATE_KEY.search(data):
        out.append("contains a private-key block")
    fixture = rel in KNOWN_FIXTURES
    if not fixture and TOKENS.search(data):
        out.append("contains a token-shaped string")
    for m in mk:
        if m in data:
            out.append("contains the local user's home path or another configured marker")
            break
    if not fixture and cst.PREFILTER.search(data):
        for i, line in enumerate(data.decode("utf-8", "replace").splitlines(), 1):
            if any(s.search(line) for s in cst.SHAPES):
                out.append(f"line {i}: credential-shaped text")
                break
    return out


def git_lines(root, *args):
    r = subprocess.run(["git", "-C", root, *args], capture_output=True)
    if r.returncode != 0:
        raise RuntimeError(r.stderr.decode("utf-8", "replace").strip())
    return [x for x in r.stdout.decode("utf-8", "surrogateescape").split("\0") if x]


def history_findings(root):
    out = []
    names = subprocess.run(["git", "-C", root, "log", "--all", "--name-only", "--format=commit %H"], capture_output=True).stdout.decode("utf-8", "replace")
    commit = ""
    for l in names.splitlines():
        if l.startswith("commit "):
            commit = l[7:19]
        elif l.strip():
            base = l.rsplit("/", 1)[-1]
            if l.lower().endswith(FORBIDDEN_SUFFIX) or FORBIDDEN_NAME.match(base) or l.split("/")[0] in FORBIDDEN_DIRS:
                out.append(f"history {commit}: {l}: forbidden path")
    patch = subprocess.run(["git", "-C", root, "log", "--all", "-p", "--no-color", "--format=commit %H"], capture_output=True).stdout
    commit = ""
    for l in patch.split(b"\n"):
        if l.startswith(b"commit "):
            commit = l[7:19].decode()
        elif l.startswith(b"+") and (PRIVATE_KEY.search(l) or TOKENS.search(l) or any(s.search(l.decode("utf-8", "replace")) for s in cst.SHAPES)):
            out.append(f"history {commit}: added line with secret-shaped content (value not shown)")
    return out


def main(argv):
    use_git, hist, as_list = "--git" in argv, "--history" in argv, "--list" in argv
    root = ROOT
    rules = load_rules(root)
    cands, skipped = walk_candidates(root, rules)
    problems = []  # (path, message)
    tracked_ignored = []
    if use_git:
        try:
            tracked = git_lines(root, "ls-files", "-z")
            others = git_lines(root, "ls-files", "-z", "--others", "--exclude-standard")
        except (RuntimeError, OSError) as e:
            print(f"prepublish_check: Git is not usable here: {e}", file=sys.stderr)
            return 2
        for t in tracked:
            if ignored(rules, t, False) or any(ignored(rules, "/".join(t.split("/")[:i]), True) for i in range(1, len(t.split("/")))):
                tracked_ignored.append(t)
        cands = sorted(set(tracked) | set(others) | set(cands))
        for t in tracked_ignored:
            problems.append((t, "tracked by Git although .gitignore ignores it: `git rm --cached -- <path>` (the file stays on disk)"))
    if as_list:
        print("\n".join(cands))
        return 0
    mk = markers()
    total = 0
    for rel in cands:
        for msg in check_file(root, rel, mk):
            problems.append((rel, msg))
        try:
            total += os.lstat(os.path.join(root, rel)).st_size
        except OSError:
            pass
    if total > MAX_TOTAL:
        problems.append(("(all candidate files)", f"together {total // 1024} KiB, more than the {MAX_TOTAL // 1024} KiB limit - something large was added"))
    hist_n = 0
    if hist:
        if not use_git:
            print("prepublish_check: --history needs --git", file=sys.stderr)
            return 2
        for m in history_findings(root):
            problems.append(("(history)", m))
        hist_n = 1
    for path, msg in problems:
        print(f"VIOLATION {path}: {msg}")
    kinds = {}
    for rel in skipped:
        kinds[skipped[rel]] = kinds.get(skipped[rel], 0) + 1
    ign_top = sorted({k.split("/")[0] for k in skipped})
    print(f"\nchecked {len(cands)} candidate file(s), {total // 1024} KiB in total; .gitignore excludes {len(skipped)} top-most path(s) "
          f"({kinds.get('dir', 0)} folders, {kinds.get('file', 0)} files) below {', '.join(ign_top) or '-'}")
    print("scope: " + ("filesystem + Git index (tracked and untracked-not-ignored)" if use_git else "filesystem with .gitignore applied (Git index NOT consulted: run with --git)")
          + ("; history scanned" if hist_n else "; history NOT scanned (--git --history)"))
    print("not checked: copyright/licence status, document content beyond the secret shapes, binary file internals other than magic bytes")
    print(f"{len(problems)} violation(s)" if problems else "no violation found within that scope")
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
