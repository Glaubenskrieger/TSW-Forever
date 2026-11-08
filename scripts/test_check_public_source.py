#!/usr/bin/env python3
"""Public snapshot auditing must not create an export or accept private payloads."""
import io
from pathlib import Path
import sys
import tarfile
import tempfile
import unittest
sys.path.insert(0, str(Path(__file__).resolve().parent))
import check_public_source as audit
from test_check_release_set import make_src


class Snapshot(unittest.TestCase):
    def audit(self, extra=None, link=False, drop=None):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / 'fixture.tar.gz'
            make_src(p, '0.2.0-rc.1', drop=(drop,) if drop else ())
            if extra:
                with tarfile.open(p) as t:
                    existing = [(m, t.extractfile(m).read()) for m in t.getmembers() if m.isfile()]
                with tarfile.open(p, 'w:gz') as t:
                    for m, data in existing: t.addfile(m, io.BytesIO(data))
                    name, data = extra; m = tarfile.TarInfo('tsw-forever-0.2.0-rc.1/' + name)
                    if link:
                        m.type = tarfile.SYMTYPE; m.linkname = '/outside'
                        t.addfile(m)
                    else:
                        m.size = len(data); t.addfile(m, io.BytesIO(data))
            return audit.check(p)

    def test_clean_fixture_and_no_export(self):
        self.assertEqual(self.audit(), [])

    def test_private_history_notes_branding_and_unapproved_scripts_are_refused(self):
        for name in ['.git/config', 'reports/private.txt', 'assets/branding/image.png', 'documentation/project-status.md', 'scripts/private-helper.py', 'scripts/rogue.ps1', 'documentation/windows-development-progress.md', 'packaging/windows/setup.exe']:
            self.assertTrue(self.audit((name, b'fixture')), name)

    def test_the_directory_entry_of_an_approved_root_file_is_accepted_but_other_root_directories_are_not(self):
        with tempfile.TemporaryDirectory() as tmp:
            for dirname, expect_ok in (('.cargo', True), ('.secret', False)):
                p = Path(tmp) / (dirname.strip('.') + '.tar.gz')
                make_src(p, '0.2.0-rc.1')
                with tarfile.open(p) as t:
                    existing = [(m, t.extractfile(m).read()) for m in t.getmembers() if m.isfile()]
                with tarfile.open(p, 'w:gz') as t:
                    for m, data in existing: t.addfile(m, io.BytesIO(data))
                    d = tarfile.TarInfo('tsw-forever-0.2.0-rc.1/' + dirname); d.type = tarfile.DIRTYPE; t.addfile(d)
                errs = audit.check(p)
                self.assertEqual([e for e in errs if dirname in e], [] if expect_ok else [e for e in errs if dirname in e] or ['missing'], dirname)
                self.assertTrue(expect_ok or any(dirname in e for e in errs), dirname)

    def test_paths_links_binary_and_credentials_are_refused(self):
        for name, data in [('launcher/../private.txt', b'fixture'), ('launcher/payload.exe', b'MZfixture'), ('launcher/leak.txt', b'api_' + b'key: ' + b'SYNTHETIC_TEST_VALUE')]:
            self.assertTrue(self.audit((name, data)), name)
        self.assertTrue(self.audit(('launcher/link', b''), link=True))

    def test_missing_source_and_mandatory_notice_fail(self):
        for name in ['launcher-gui/ui/launcher.slint', 'LICENSES/inventory.json', 'LICENSE']:
            self.assertTrue(self.audit(drop=name), name)


if __name__ == '__main__':
    unittest.main()
