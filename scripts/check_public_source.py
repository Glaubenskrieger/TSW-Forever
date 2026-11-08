#!/usr/bin/env python3
"""Audit a future public source snapshot in memory. Never creates an export directory."""
from pathlib import Path, PurePosixPath
import sys
import tarfile

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_licenses
import check_release_set
import check_release_artifacts as artifacts
import check_secrets_in_tree as secrets
import prepublish_check as prepublish

ALLOWED = {'launcher', 'launcher-gui', 'patcher', 'packaging', 'scripts', 'documentation', 'LICENSES'}
ROOT_FILES = {'LICENSE', 'THIRD-PARTY-NOTICES.md', 'README.md', 'CHANGELOG.md', 'rustfmt.toml', '.cargo/config.toml'}
ROOT_DIRS = {str(PurePosixPath(f).parent) for f in ROOT_FILES if '/' in f}  # the directory entry of an approved root file (.cargo/)
DOCS = {'building-from-source.md', 'user-guide.md', 'public-export.md', 'microsoft-components.md', 'architecture.md', 'packaging.md', 'security.md', 'third-party-components.md', 'versioning-policy.md', 'building-windows.md'}
SCRIPTS = {'version.py', 'install_native.sh', 'check_licenses.py', 'build_flatpak.sh', 'check_flatpak_package.py', 'build_windows_installer.ps1', 'test_windows_installer.ps1'}


def check(path):
    errors = []
    files = {}
    with tarfile.open(path, 'r:gz') as archive:
        members = archive.getmembers()
        tops = {m.name.split('/')[0] for m in members}
        if len(tops) != 1 or not next(iter(tops)).startswith('tsw-forever-'):
            return ['source must have one versioned top directory']
        seen = set()
        total = 0
        for member in members:
            parts = PurePosixPath(member.name).parts
            if member.name.startswith('/') or '..' in parts or member.name in seen or '\\' in member.name or any(ord(c) < 32 for c in member.name):
                errors.append(f'unsafe/duplicate source path: {member.name}')
                continue
            seen.add(member.name)
            if not member.isdir() and not member.isfile():
                errors.append(f'source links/special files forbidden: {member.name}')
                continue
            if len(parts) == 1:
                continue
            rel = '/'.join(parts[1:])
            if parts[1] not in ALLOWED and rel not in ROOT_FILES and not (member.isdir() and rel in ROOT_DIRS):
                errors.append(f'private/unapproved source root: {rel}')
            if any(p in prepublish.FORBIDDEN_DIRS for p in parts[1:]):
                errors.append(f'private source directory: {rel}')
            if not member.isfile():
                continue
            if parts[1] == 'documentation' and '/'.join(parts[2:]) not in DOCS:
                errors.append(f'unapproved documentation: {rel}')
            if parts[1] == 'scripts' and '/'.join(parts[2:]) not in SCRIPTS:
                errors.append(f'unapproved script: {rel}')
            if member.size > prepublish.MAX_FILE:
                errors.append(f'oversize source file: {rel}')
                continue
            total += member.size
            if total > prepublish.MAX_TOTAL:
                errors.append("source exceeds the total publication size limit")
                break
            data = archive.extractfile(member).read()
            files[rel] = data
            if artifacts.FORBIDDEN_NAME.search(rel) or any(data.startswith(magic) for magic, _ in prepublish.MAGIC):
                errors.append(f'proprietary/binary/private source content: {rel}')
            if prepublish.PRIVATE_KEY.search(data) or prepublish.TOKENS.search(data):
                errors.append(f'credential material: {rel}')
            if b'\x00' not in data[:4096]:
                for number, line in enumerate(data.decode('utf-8', 'replace').splitlines(), 1):
                    if any(shape.search(line) for shape in secrets.SHAPES) and secrets.line_id(line) not in secrets.REVIEWED_LINES.get(rel, ()):
                        errors.append(f'credential-shaped line: {rel}:{number}')
        for rel in check_release_set.REQUIRED_IN_SOURCE:
            if rel not in files:
                errors.append(f"required public source missing: {rel}")
        errors.extend(check_licenses.check(files.__getitem__))
    return errors


if __name__ == '__main__':
    if len(sys.argv) != 2:
        raise SystemExit('usage: check_public_source.py <development-source-archive>')
    errors = check(sys.argv[1])
    for error in errors:
        print(error, file=sys.stderr)
    print('Public source snapshot audit: ' + ('FAILED' if errors else 'passed; no export performed'))
    raise SystemExit(bool(errors))
