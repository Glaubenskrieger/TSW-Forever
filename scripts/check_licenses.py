#!/usr/bin/env python3
"""Validate the release licence inventory without network access or generated substitutes."""
import hashlib
import json
from pathlib import Path, PurePosixPath
import sys
import subprocess
import tomllib

ROOT = Path(__file__).resolve().parent.parent


def check(read):
    errors = []
    try:
        inventory = json.loads(read('LICENSES/inventory.json'))
        lock = tomllib.loads(read('launcher-gui/Cargo.lock').decode())
        locked = {(p['name'], p['version']): p for p in lock['package']}
        if inventory['format'] != 1 or inventory['target'] != 'x86_64-unknown-linux-gnu':
            raise ValueError('unsupported inventory format or target')
        if not inventory['components']:
            raise ValueError('empty component inventory')
        seen = set()
        notices = list(inventory['additional_notices'])
        for component in inventory['components']:
            key = (component['name'], component['version'])
            if key in seen or key not in locked:
                raise ValueError(f'duplicate or unlocked component {key}')
            seen.add(key)
            if component['source_sha256'] != locked[key].get('checksum'):
                raise ValueError(f'source checksum differs from Cargo.lock: {key}')
            url = f'https://static.crates.io/crates/{key[0]}/{key[0]}-{key[1]}.crate'
            if component['source_url'] != url or not component['notices'] or not component['license']:
                raise ValueError(f'missing source/licence information: {key}')
            notices.extend(component['notices'])
        paths = {n['path'] for n in notices}
        names = {name for name, _ in seen}
        font_requirements = set()
        if 'sctk-adwaita' in names:
            font_requirements.update({'LICENSES/fonts/Cantarell-copyright.txt', 'LICENSES/fonts/OFL-1.1.txt'})
        if 'i-slint-common' in names:
            font_requirements.add('LICENSES/fonts/OFL-1.1-RFN.txt')
            if not any(path.endswith('/sharedfontique/Inter-VariableFont.ttf.license') for path in paths):
                errors.append('missing Inter font copyright notice')
        for path in sorted(font_requirements - paths):
            errors.append(f'missing mandatory font licence: {path}')
        for item in notices:
            path = PurePosixPath(item['path'])
            if path.is_absolute() or '..' in path.parts or path.parts[0] != 'LICENSES' or not item['origin']:
                raise ValueError('unsafe or unattributed notice path')
            data = read(str(path))
            if not data.strip() or hashlib.sha256(data).hexdigest() != item['sha256']:
                errors.append(f'missing/changed mandatory licence text: {path}')
        for name in ['LICENSE', 'THIRD-PARTY-NOTICES.md', 'LICENSES/README.md']:
            if not read(name).strip():
                errors.append(f'empty licence documentation: {name}')
    except (OSError, KeyError, ValueError, TypeError) as e:
        errors.append(f'licence inventory invalid or mandatory file missing: {e}')
    return errors


def check_tree(root):
    def read(rel):
        p = root / rel
        if p.is_symlink():
            raise ValueError(f'licence symlink refused: {rel}')
        return p.read_bytes()
    return check(read)


def check_closure(root):
    output = subprocess.check_output(['cargo', 'tree', '--offline', '--locked', '--manifest-path', str(root / 'launcher-gui/Cargo.toml'),
        '--target', 'x86_64-unknown-linux-gnu', '--edges', 'normal,build', '--prefix', 'none', '--format', '{p}'], text=True)
    data = json.loads((root / 'LICENSES/inventory.json').read_text())
    required = set()
    for line in output.splitlines():
        fields = line.split()
        if len(fields) > 1 and fields[1].startswith('v') and not any(f.startswith('(') and '/' in f for f in fields[2:]):
            required.add((fields[0], fields[1][1:]))
    supplied = {(c['name'], c['version']) for c in data['components']}
    return [f'Linux dependency licence inventory differs: missing {sorted(required - supplied)}, obsolete {sorted(supplied - required)}'] if required != supplied else []


def main():
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else ROOT
    errors = check_tree(root)
    if not errors and (root / "launcher-gui/Cargo.toml").is_file():
        errors.extend(check_closure(root))
    for error in errors:
        print(error, file=sys.stderr)
    print('Licence inventory: ' + ('FAILED' if errors else 'complete, pinned texts verified'))
    return bool(errors)


if __name__ == '__main__':
    sys.exit(main())
