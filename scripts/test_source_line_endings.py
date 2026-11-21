#!/usr/bin/env python3
"""Guard against a class of Windows-only test failures (0.4.1: update_paths.rs failed in the Windows CI, never on Linux).

A Windows checkout may hold the sources with CRLF line endings (there is no .gitattributes), and `include_str!` embeds them unchanged. A source-level test that looks for a
MULTI-LINE shape in such a text (for example the closing "\\n}\\n" of a function, or "#[cfg(test)]\\nmod tests") then finds nothing on Windows. Every Rust file that embeds source text
with `include_str!` and searches it with a pattern containing a line break must therefore normalise CRLF to LF first.

This test needs no Windows: it reads the Rust files as text. The behaviour itself is covered by scripts/test_crlf_checkout.sh (a CRLF copy of the tree is tested)."""
import os
import re
import unittest

HERE = os.path.dirname(os.path.realpath(__file__))
ROOT = os.path.realpath(os.path.join(HERE, ".."))
CRATES = ("launcher", "launcher-gui", "patcher")
SKIP_DIRS = {"target", "build", "vendor", "__pycache__"}

INCLUDE = re.compile(r"include_str!\(")
# a search that needs a line break: find/rfind/split/contains/matches/starts_with/strip_prefix("...\n...")
MULTILINE_SEARCH = re.compile(r"\.(?:find|rfind|split|rsplit|contains|matches|starts_with|ends_with|strip_prefix|strip_suffix)\(\s*&?\s*\"(?:[^\"\\]|\\.)*\\n(?:[^\"\\]|\\.)*\"")
NORMALISES = re.compile(r'replace\(\s*"\\r\\n"\s*,\s*"\\n"\s*\)')


def rust_files():
    for crate in CRATES:
        for d, dirs, files in os.walk(os.path.join(ROOT, crate)):
            dirs[:] = [x for x in dirs if x not in SKIP_DIRS]
            for f in files:
                if f.endswith(".rs"):
                    yield os.path.join(d, f)


class SourceLineEndings(unittest.TestCase):
    def test_multi_line_searches_in_embedded_sources_normalise_crlf(self):
        bad = []
        for path in rust_files():
            text = open(path, encoding="utf-8").read()
            if INCLUDE.search(text) and MULTILINE_SEARCH.search(text) and not NORMALISES.search(text):
                bad.append(os.path.relpath(path, ROOT))
        self.assertEqual(bad, [], f"{bad}: embeds source text and searches it for a line break without normalising CRLF to LF first (see this file's docstring)")

    def test_the_guard_recognises_the_failing_shape(self):
        failing = 'const M: &str = include_str!("../src/main.rs");\nfn f() { let e = M.find("\\n}\\n").unwrap(); }\n'
        fixed = 'fn lf(t: &str) -> String { t.replace("\\r\\n", "\\n") }\n' + failing
        harmless = 'const M: &str = include_str!("../src/main.rs");\nfn f() { assert!(M.contains("fn main")); }\n'
        self.assertTrue(INCLUDE.search(failing) and MULTILINE_SEARCH.search(failing) and not NORMALISES.search(failing))
        self.assertTrue(NORMALISES.search(fixed))
        self.assertFalse(MULTILINE_SEARCH.search(harmless))
        self.assertTrue(MULTILINE_SEARCH.search('x.split("#[cfg(test)]\\nmod tests")'))


class CrlfCheckoutScript(unittest.TestCase):
    """scripts/test_crlf_checkout.sh copies sources only (never the whole working tree, which in the private repository holds analysis data and game files) and writes below build/ only."""

    def setUp(self):
        self.text = open(os.path.join(HERE, "test_crlf_checkout.sh"), encoding="utf-8").read()

    def test_it_copies_an_explicit_source_list_and_nothing_else(self):
        self.assertIn("--exclude='*'", self.text, "everything that is not listed is excluded")
        self.assertIn("--include='/launcher-gui/src/***'", self.text)
        for forbidden in ("analysis", "reports", "logs", "research", "native", "tools", "game", "drive_c", "--include='*'"):
            self.assertNotIn(forbidden, self.text.split("rsync -a", 1)[1].split('"$root/"', 1)[0], forbidden)

    def test_it_writes_below_build_only_at_a_fixed_path_with_its_own_target_directory(self):
        self.assertNotIn("mktemp", self.text)
        code = "\n".join(l for l in self.text.splitlines() if not l.lstrip().startswith("#"))
        self.assertNotRegex(code, r"(^|[^$\w])/tmp\b", "no write to /tmp")
        self.assertIn('copy="$root/build/crlf-checkout"', self.text)
        self.assertIn('export CARGO_TARGET_DIR="${TSW_CRLF_TARGET_DIR:-$root/build/crlf-target}"', self.text, "its own build cache: build scripts bake their location in")
        self.assertIn('"$root/build/"*', self.text, "refuses a destination outside build/")


if __name__ == "__main__":
    unittest.main()
