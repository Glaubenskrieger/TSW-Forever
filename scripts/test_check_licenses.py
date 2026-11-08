#!/usr/bin/env python3
"""Licence audit regressions: deletion and mutation must fail, not silently degrade."""
import hashlib
import json
from pathlib import Path
import sys
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_licenses as audit


class Licences(unittest.TestCase):
    def files(self):
        notice = b'Synthetic test licence notice\n'
        component = {'name': 'fixture', 'version': '1.0.0', 'license': 'MIT', 'source_sha256': 'a' * 64,
            'source_url': 'https://static.crates.io/crates/fixture/fixture-1.0.0.crate',
            'notices': [{'path': 'LICENSES/text.txt', 'sha256': hashlib.sha256(notice).hexdigest(), 'origin': 'synthetic fixture'}]}
        return {'LICENSES/inventory.json': json.dumps({'format': 1, 'target': 'x86_64-unknown-linux-gnu', 'components': [component], 'additional_notices': []}).encode(),
            'launcher-gui/Cargo.lock': ('[[package]]\nname="fixture"\nversion="1.0.0"\nchecksum="'+'a'*64+'"\n').encode(),
            'LICENSE': b'fixture', 'THIRD-PARTY-NOTICES.md': b'fixture', 'LICENSES/README.md': b'fixture', 'LICENSES/text.txt': notice}

    def test_complete_inventory_and_each_missing_mandatory_text(self):
        files = self.files()
        self.assertEqual(audit.check(files.__getitem__), [])
        for path in files:
            reduced = dict(files); del reduced[path]
            self.assertTrue(audit.check(reduced.__getitem__), path)

    def test_modified_text_wrong_source_checksum_and_unlocked_version_fail(self):
        for path, replacement in [('LICENSES/text.txt', b'changed'), ('launcher-gui/Cargo.lock', b'[[package]]\nname="fixture"\nversion="2.0.0"\n')]:
            files = self.files(); files[path] = replacement
            self.assertTrue(audit.check(files.__getitem__))

    def test_traversing_notice_empty_component_set_and_duplicate_component_fail(self):
        for change in ['path', 'empty', 'duplicate']:
            files = self.files(); data = json.loads(files['LICENSES/inventory.json'])
            if change == 'path': data['components'][0]['notices'][0]['path'] = 'LICENSES/../private.txt'
            if change == 'empty': data['components'] = []
            if change == 'duplicate': data['components'] *= 2
            files['LICENSES/inventory.json'] = json.dumps(data).encode()
            self.assertTrue(audit.check(files.__getitem__))

    def test_required_font_notices_cannot_be_removed_from_inventory(self):
        original = json.loads((audit.ROOT / 'LICENSES/inventory.json').read_text())
        for removed in ['LICENSES/fonts/OFL-1.1.txt', 'LICENSES/fonts/OFL-1.1-RFN.txt']:
            data = json.loads(json.dumps(original))
            data['additional_notices'] = [n for n in data['additional_notices'] if n['path'] != removed]
            def read(rel):
                return json.dumps(data).encode() if rel == 'LICENSES/inventory.json' else (audit.ROOT / rel).read_bytes()
            self.assertTrue(any('font' in error for error in audit.check(read)), removed)

    def test_actual_inventory_matches_linux_dependency_closure(self):
        self.assertEqual(audit.check_tree(audit.ROOT), [])
        self.assertEqual(audit.check_closure(audit.ROOT), [])


if __name__ == '__main__':
    unittest.main()
