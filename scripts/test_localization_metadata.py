#!/usr/bin/env python3
"""Parity of every place that carries a language: the window catalogues, the Linux desktop entry, the AppStream metadata and the Windows installer.

The launcher's list of languages (`LANGUAGES` in launcher-gui/src/i18n.rs) is the single source. Every other file must offer exactly the same set, with the same
placeholders, so that Linux and Windows stay equal (documentation/internationalization.md). Runs without Inno Setup, Flatpak or Windows; what only the Inno Setup
compiler or a Windows machine can check (the compile of the installer, its rendered pages) is covered by the Windows build and the installer tests.
"""
import os
import re
import sys
import unittest
import xml.etree.ElementTree as ET

ROOT = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))


def read(*parts):
    with open(os.path.join(ROOT, *parts), encoding="utf-8") as f:
        return f.read()


I18N = read("launcher-gui", "src", "i18n.rs")
LANGS = re.findall(r'^\s*\("([A-Za-z-]+)", "([^"]+)", ', I18N, re.M)  # (code, own name)
CODES = [c for c, _ in LANGS]
NON_EN = [c for c in CODES if c != "en"]
# the agreed set, in order (owner decision): a change here is a product decision
AGREED = ["en", "de", "fr", "es", "pt-BR", "pl"]
# code -> (Inno Setup language name, official message file, desktop/AppStream locale)
MAP = {
    "en": ("english", "Default.isl", None),
    "de": ("german", "Languages\\German.isl", "de"),
    "fr": ("french", "Languages\\French.isl", "fr"),
    "es": ("spanish", "Languages\\Spanish.isl", "es"),
    "pt-BR": ("brazilianportuguese", "Languages\\BrazilianPortuguese.isl", "pt_BR"),
    "pl": ("polish", "Languages\\Polish.isl", "pl"),
}


def mojibake(text):
    """True when the text looks like UTF-8 that was read as Latin-1/CP1252 once too often (`Ã¤` for `ä`, `Ä\x85` for `ą`)."""
    for enc in ("latin-1", "cp1252"):
        try:
            if text.encode(enc) != text.encode("utf-8") and text.encode(enc).decode("utf-8") != text:
                return True
        except (UnicodeEncodeError, UnicodeDecodeError):
            continue
    return False


def placeholders(text):
    return sorted(re.findall(r"%[0-9n%]|\{\d*\}|\{[a-z_]+\}", text))


class WindowCatalogues(unittest.TestCase):
    def test_the_registered_languages_are_the_agreed_six(self):
        self.assertEqual(CODES, AGREED)
        self.assertEqual([n for _, n in LANGS], ["English", "Deutsch", "Français", "Español", "Português (Brasil)", "Polski"])

    def po(self, code):
        text = read("launcher-gui", "translations", code, "LC_MESSAGES", "tsw-launcher-gui.po")
        pairs = re.findall(r'^msgid "(.*)"\nmsgstr "(.*)"$', text, re.M)
        return [(i, s) for i, s in pairs if i], text

    def test_every_catalogue_has_the_same_ids_in_the_same_order_with_the_same_placeholders(self):
        base, _ = self.po("de")
        self.assertGreater(len(base), 300)
        for code in NON_EN:
            pairs, text = self.po(code)
            self.assertIn(f'"Language: {code}\\n"', text, code)
            self.assertEqual([i for i, _ in pairs], [i for i, _ in base], f"{code}: message ids")
            for (i, s) in pairs:
                self.assertTrue(s.strip(), f"{code}: empty translation of {i!r}")
                self.assertEqual(placeholders(i), placeholders(s), f"{code}: placeholders of {i!r}")

    def test_the_catalogues_are_utf8_without_a_bom_and_use_lf(self):
        for code in NON_EN:
            raw = open(os.path.join(ROOT, "launcher-gui", "translations", code, "LC_MESSAGES", "tsw-launcher-gui.po"), "rb").read()
            self.assertFalse(raw.startswith(b"\xef\xbb\xbf"), code)
            self.assertNotIn(b"\r", raw, code)
            raw.decode("utf-8")

    def test_polish_special_characters_are_present_and_intact(self):
        text = read("launcher-gui", "translations", "pl", "LC_MESSAGES", "tsw-launcher-gui.po")
        # "Ź" (capital, rare: it started only the removed text about the update source) is not required any more; the lower-case ź stays checked
        for ch in "ąćęłńóśźżĄĆĘŁÓŚ":
            self.assertIn(ch, text, ch)
        self.assertNotIn("�", text)
        self.assertFalse(mojibake(text), "double-encoded characters")


class DesktopEntry(unittest.TestCase):
    def entries(self):
        d = {}
        for line in read("launcher-gui", "packaging", "linux", "org.tswlauncher.Launcher.desktop").split("\n"):
            m = re.match(r"^([A-Za-z]+)(?:\[([A-Za-z_@]+)\])?=(.*)$", line)
            if m:
                d[(m.group(1), m.group(2))] = m.group(3)
        return d

    def test_name_generic_name_and_comment_exist_in_every_language(self):
        d = self.entries()
        for code in NON_EN:
            loc = MAP[code][2]
            for key in ("Name", "GenericName", "Comment"):
                self.assertTrue(d.get((key, loc)), f"{key}[{loc}] missing")
        # no translation for a language the launcher does not offer
        offered = {MAP[c][2] for c in NON_EN}
        for (key, loc) in d:
            if loc is not None:
                self.assertIn(loc, offered, f"{key}[{loc}] has no matching launcher language")
        # the brand name is never translated
        for code in NON_EN:
            self.assertEqual(d[("Name", MAP[code][2])], d[("Name", None)])


class AppStream(unittest.TestCase):
    XML_LANG = "{http://www.w3.org/XML/1998/namespace}lang"

    def test_summary_and_both_description_paragraphs_are_translated_into_every_language(self):
        root = ET.fromstring(read("packaging", "flatpak", "org.tswlauncher.Launcher.metainfo.xml"))
        want = {MAP[c][2] for c in NON_EN}
        summaries = {e.get(self.XML_LANG): e.text for e in root.findall("summary") if e.get(self.XML_LANG)}
        self.assertEqual(set(summaries), want)
        paragraphs = root.find("description").findall("p")
        base = [p for p in paragraphs if p.get(self.XML_LANG) is None]
        self.assertEqual(len(base), 2)
        for loc in want:
            tr = [p for p in paragraphs if p.get(self.XML_LANG) == loc]
            self.assertEqual(len(tr), len(base), f"description paragraphs in {loc}")
        # every translation is real text, not a copy of the English one
        for e in root.findall("summary"):
            if e.get(self.XML_LANG):
                self.assertNotEqual(e.text, root.find("summary").text)
        for p in paragraphs:
            if p.get(self.XML_LANG):
                self.assertNotIn(p.text, [b.text for b in base])


class InstallerLanguages(unittest.TestCase):
    ISS = read("packaging", "windows", "tsw-forever.iss")

    def languages(self):
        return re.findall(r'^Name: "(\w+)"; MessagesFile: "compiler:([^"]+)"$', self.ISS, re.M)

    def messages(self):
        out = {}
        sect = self.ISS.split("[CustomMessages]")[1].split("\n[")[0]
        for m in re.finditer(r"^(\w+)\.(\w+)=(.*)$", sect, re.M):
            out.setdefault(m.group(1), {})[m.group(2)] = m.group(3)
        return out

    def test_the_installer_offers_exactly_the_languages_of_the_launcher_with_official_inno_files(self):
        got = self.languages()
        self.assertEqual(got, [(MAP[c][0], MAP[c][1]) for c in AGREED])

    def test_every_language_has_the_same_custom_message_keys_and_placeholders_as_english(self):
        msgs = self.messages()
        self.assertEqual(list(msgs), [MAP[c][0] for c in AGREED])
        english = msgs["english"]
        self.assertEqual(len(english), 13)
        for lang, m in msgs.items():
            self.assertEqual(set(m), set(english), f"{lang}: keys")
            for key, text in m.items():
                self.assertTrue(text.strip(), f"{lang}.{key} is empty")
                self.assertEqual(placeholders(english[key]), placeholders(text), f"{lang}.{key}: placeholders (%1 %2 %n)")
                self.assertNotIn("�", text)
                self.assertFalse(mojibake(text), f"{lang}.{key}: double-encoded characters")
        # a command line option stays exactly as it is
        for lang, m in msgs.items():
            self.assertIn("/ALLOWDOWNGRADE=1", m["DowngradeRefused"], lang)
            self.assertNotIn("/ALLOWDOWNGRADE=1", m["DowngradeConfirm"], lang)

    def test_every_key_the_installer_code_uses_exists_and_no_key_is_unused(self):
        code = self.ISS.split("\n[Code]\n")[1]
        used = set(re.findall(r"CustomMessage\('(\w+)'\)", code))
        defined = set(self.messages()["english"])
        self.assertEqual(used, defined)

    def test_the_script_is_utf8_with_bom_which_inno_setup_needs_for_accents(self):
        raw = open(os.path.join(ROOT, "packaging", "windows", "tsw-forever.iss"), "rb").read()
        self.assertTrue(raw.startswith(b"\xef\xbb\xbf"), "without the BOM Inno Setup reads the file as ANSI and the accents break")
        raw.decode("utf-8")

    def test_the_buttons_stay_short_enough_for_the_wizard(self):
        for lang, m in self.messages().items():
            for key in ("ButtonUpdate", "ButtonRepair", "ButtonDowngrade"):
                self.assertLessEqual(len(m[key]), 24, f"{lang}.{key}")


if __name__ == "__main__":
    unittest.main(verbosity=1)
