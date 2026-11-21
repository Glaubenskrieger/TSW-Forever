#!/usr/bin/env python3
"""Checks of the web site sources (website/): colours, six translations, no external resources, and that the Pages assembler accepts the folder.

* the colours are the launcher's own (website/css/themes.css is regenerated from launcher-gui/src/theme.rs and compared);
* the six languages of the launcher exist, have exactly the keys of English, no empty value and no placeholder; a translated text is not a copy of the English one
  (except for the few names and terms that are the same in the language);
* every text of the page has a key and the English text in the HTML is the English dictionary text (the page reads correctly without JavaScript);
* nothing is loaded from outside: no external script, style, font or image; links go to the project's own GitHub pages only;
* nothing in the folder can collide with the Flatpak repository files of the Pages site (scripts/assemble_pages_site.py accepts it), no symbolic link, no key or secret shape;
* the JavaScript files parse (node --check, where node exists).
"""
import html
import json
import os
import re
import shutil
import subprocess
import sys
import tempfile
import unittest
import urllib.parse

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.abspath(os.path.join(HERE, ".."))
SITE = os.path.join(ROOT, "website")
sys.path.insert(0, HERE)
import assemble_pages_site as pages  # noqa: E402
import gen_website_themes as themes  # noqa: E402

LANGUAGES = ["en", "de", "fr", "es", "pt-BR", "pl"]
# Texts that are legitimately the same as the English one in some languages: names, product words and loanwords. Everything else must be translated.
SAME_AS_ENGLISH_OK = {
    "meta.title", "footer.text", "faction.illuminati.name", "faction.dragon.name", "dl.win.title", "updates.win.title", "install.win.title", "nav.downloads", "hero.cta", "dl.title",
    "ui.faction", "dl.src.title", "nav.label", "faction.templars.name", "docs.readme", "docs.title", "legal.title", "ui.language", "nav.docs", "updates.title", "docs.changelog", "about.open.title",
}
# keys that only the scripts use (messages, the copy button states, the download note that js/download.js fills in)
SCRIPT_KEYS = {"msg.storage", "ui.copied", "ui.copyfail", "dl.win.status.found", "dl.win.status.failed"}
ALLOWED_LINKS = re.compile(r"^https://github\.com/Glaubenskrieger/TSW-Forever(/|#|$)")
PLACEHOLDER = re.compile(r"\btodo\b(?! (lo|el|la))|\btbd\b|lorem|xxx|translate me|\?\?\?", re.I)


def read(*p):
    with open(os.path.join(SITE, *p), encoding="utf-8") as f:
        return f.read()


def dictionary(lang):
    text = read("js", "i18n", f"{lang}.js")
    m = re.search(r"window\.TSW_I18N\[\"" + re.escape(lang) + r"\"\]\s*=\s*(\{.*\});\s*$", text, re.S)
    assert m, f"{lang}.js: no dictionary found"
    return json.loads(m.group(1))


def page_keys():
    page = read("index.html")
    keys = set(re.findall(r'data-i18n="([^"]+)"', page))
    for attrs in re.findall(r'data-i18n-attr="([^"]+)"', page):
        for pair in attrs.split(";"):
            keys.add(pair.split(":", 1)[1])
    return keys


class Colours(unittest.TestCase):
    def test_the_theme_file_is_generated_from_the_launchers_own_theme_table(self):
        self.assertEqual(themes.main(["--check"]), 0)

    def test_the_three_factions_are_there_with_all_values(self):
        css = read("css", "themes.css")
        for tid in themes.FACTIONS:
            self.assertIn(f'[data-theme="{tid}"]', css)
        self.assertEqual(css.count("--ornament:"), 3)
        self.assertEqual(css.count("--ink:"), 3)

    def test_templars_are_the_default_without_javascript(self):
        self.assertIn('<html lang="en" data-theme="templars">', read("index.html"))
        self.assertIn(":root,", read("css", "themes.css"))


class Translations(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.d = {lang: dictionary(lang) for lang in LANGUAGES}

    def test_the_languages_are_exactly_those_of_the_launcher(self):
        cfg = read("js", "config.js")
        self.assertEqual(re.search(r"languages: \[([^\]]*)\]", cfg).group(1).replace('"', "").replace(" ", "").split(","), LANGUAGES)
        options = re.findall(r'<option value="([^"]+)">', read("index.html"))
        self.assertEqual(options, LANGUAGES)
        for lang in LANGUAGES:
            self.assertIn(f'<script src="js/i18n/{lang}.js">', read("index.html"))

    def test_every_language_has_exactly_the_keys_of_english(self):
        english = set(self.d["en"])
        for lang in LANGUAGES[1:]:
            self.assertEqual(set(self.d[lang]) ^ english, set(), lang)

    def test_no_empty_value_and_no_placeholder(self):
        for lang, d in self.d.items():
            for key, value in d.items():
                self.assertTrue(value.strip(), f"{lang}:{key} is empty")
                self.assertFalse(PLACEHOLDER.search(value), f"{lang}:{key} looks like a placeholder: {value!r}")
                self.assertNotEqual(value, key, f"{lang}:{key} is the key itself")

    def test_a_translation_is_not_a_copy_of_the_english_text(self):
        english = self.d["en"]
        for lang in LANGUAGES[1:]:
            same = [k for k, v in self.d[lang].items() if v == english[k] and k not in SAME_AS_ENGLISH_OK]
            self.assertEqual(same, [], f"{lang}: texts that are still English: {same}")
            # long texts are never identical, whatever the list says
            self.assertEqual([k for k, v in self.d[lang].items() if v == english[k] and len(v) > 45], [], lang)

    def test_every_text_of_the_page_has_a_key_and_the_english_html_text_is_the_dictionary_text(self):
        keys = page_keys()
        english = self.d["en"]
        self.assertEqual(keys - set(english), set(), "keys used by the page but missing in en.js")
        self.assertEqual(set(english) - keys - SCRIPT_KEYS, set(), "keys in en.js that the page never uses")
        page = read("index.html")
        for key, text in re.findall(r'data-i18n="([^"]+)"[^>]*>([^<]*)<', page):
            self.assertEqual(html.unescape(text), english[key], f"HTML default text of {key} differs from en.js")

    def test_the_page_mentions_no_unconfirmed_download_link(self):
        page = read("index.html")
        self.assertNotRegex(page, r"releases/download/")
        self.assertNotRegex(page, r"\.(exe|flatpak|tar\.gz)\"")  # file names are shown as text, never linked


class NothingExternal(unittest.TestCase):
    def test_only_local_resources_and_the_projects_own_github_pages(self):
        for dirpath, _, names in os.walk(SITE):
            for n in names:
                if not n.endswith((".html", ".css", ".js")):
                    continue
                with open(os.path.join(dirpath, n), encoding="utf-8") as f:
                    text = f.read()
                self.assertNotRegex(text, r"@import|url\(\s*[\"']?https?:", n)
                for url in re.findall(r'(?:src|href)="(https?:[^"]+)"', text):
                    self.assertRegex(url, ALLOWED_LINKS, f"{n}: external address {url}")
                self.assertNotRegex(text, r"//(cdn|fonts|ajax|unpkg)\.", n)
                self.assertNotRegex(text, r"document\.cookie|XMLHttpRequest|navigator\.sendBeacon", n)
                if n not in ("download.js", "source.js"):
                    self.assertNotRegex(text, r"fetch\(", n)
        # script and stylesheet references are relative paths
        page = read("index.html")
        for ref in re.findall(r'<(?:script|link)[^>]+(?:src|href)="([^"]+)"', page):
            self.assertFalse(ref.startswith(("http", "//", "/")), ref)

    def test_no_key_secret_or_symbolic_link_in_the_folder(self):
        for dirpath, dirs, names in os.walk(SITE):
            for n in dirs + names:
                p = os.path.join(dirpath, n)
                self.assertFalse(os.path.islink(p), p)
            for n in names:
                self.assertFalse(n.endswith((".gpg", ".key", ".pem", ".asc", ".sec")), n)
                if n.endswith((".html", ".css", ".js", ".json", ".svg")):
                    with open(os.path.join(dirpath, n), encoding="utf-8") as f:
                        text = f.read()
                    self.assertNotRegex(text, r"BEGIN (PGP|RSA|OPENSSH|EC) PRIVATE|ghp_[A-Za-z0-9]{20}|github_pat_", n)

    def test_the_icon_is_the_launchers_own_icon(self):
        with open(os.path.join(SITE, "assets", "icon.svg"), "rb") as a, open(os.path.join(ROOT, "launcher-gui", "assets", "icon.svg"), "rb") as b:
            self.assertEqual(a.read(), b.read(), "website/assets/icon.svg must stay a copy of the launcher's icon (provenance: original artwork of the project)")

    def test_the_folder_holds_only_publishable_files(self):
        top = set(os.listdir(SITE))
        self.assertTrue({"index.html", "css", "js", "assets"} <= top)
        self.assertEqual(top & (pages.RESERVED | {"tsw-forever.flatpakrepo", "org.tswlauncher.Launcher.flatpakref"}), set())
        for name in top:
            self.assertFalse(name.lower().startswith(("readme", "serve", "start")), f"a helper file would be published: {name}")


def conf():
    out = {}
    with open(os.path.join(ROOT, "packaging", "project.conf"), encoding="utf-8") as f:
        for line in f:
            if "=" in line and not line.lstrip().startswith("#"):
                k, v = line.strip().split("=", 1)
                out[k] = v
    return out


class Downloads(unittest.TestCase):
    """The Windows link follows the newest stable release; the Flatpak commands come from the project's own configuration."""

    def test_the_download_script_asks_only_the_projects_release_api_and_accepts_only_the_exact_installer(self):
        js = read("js", "download.js")
        cfg = conf()
        self.assertEqual(re.findall(r"fetch\(", js).__len__(), 1)
        self.assertIn('"https://api.github.com/repos/" + repo + "/releases/latest"', js)
        self.assertIn(f'repo: "{cfg["GITHUB_OWNER"]}/{cfg["GITHUB_REPO"]}"', read("js", "config.js"))
        self.assertIn(r"/^tsw-forever-(\d+\.\d+\.\d+)-windows-setup\.exe$/", js)
        self.assertIn(r"/^v(\d+\.\d+\.\d+)$/", js)
        self.assertIn("release.draft || release.prerelease", js)
        self.assertIn('credentials: "omit"', js)
        self.assertIn('referrerPolicy: "no-referrer"', js)
        # no version number is written into the site's settings
        self.assertNotRegex(read("js", "config.js"), r"\b\d+\.\d+\.\d+\b")
        self.assertNotRegex(read("index.html"), r"windows-setup\.exe\"")

    def test_the_fallback_button_is_the_release_page_of_the_project(self):
        page = read("index.html")
        cfg = conf()
        links = re.findall(r'class="btn primary js-win-dl" href="([^"]+)"', page)
        self.assertEqual(len(links), 2)
        self.assertEqual(set(links), {cfg["REPO_URL"] + "/releases/latest"})

    def test_the_flatpak_commands_come_from_the_project_configuration_and_are_not_translated(self):
        page = read("index.html")
        cfg = conf()
        site = cfg["FLATPAK_REPO_URL"].rsplit("/repo", 1)[0]
        with open(os.path.join(ROOT, "scripts", "gen_flatpak_refs.sh"), encoding="utf-8") as f:
            remote = re.search(r"SuggestRemoteName=(\S+)", f.read()).group(1)
        commands = [html.unescape(c) for c in re.findall(r"<pre><code>(.*?)</code></pre>", page, re.S)]
        self.assertIn(f"flatpak remote-add --user --if-not-exists {remote} {site}/tsw-forever.flatpakrepo", commands)
        self.assertIn(f"flatpak install --user {remote} {cfg['APP_ID']}//stable", commands)
        self.assertIn(f"flatpak run --user {cfg['APP_ID']}", commands)
        self.assertIn(f'href="{cfg["APP_ID"]}.flatpakref"', page)
        self.assertIn('href="tsw-forever.flatpakrepo"', page)
        self.assertNotRegex(page, r"<pre[^>]*data-i18n")
        self.assertNotRegex(page, r"<code[^>]*data-i18n")
        for c in commands:
            self.assertNotRegex(c, r"<your|\[.*\]|PLACEHOLDER|CHANGE", c)

    def test_the_gate_blocks_the_page_behind_it(self):
        js, css = read("js", "app.js"), read("css", "style.css")
        for needle in ('setAttribute("inert"', 'aria-hidden', "trapTab", 'classList.toggle("gated"'):
            self.assertIn(needle, js)
        self.assertIn("body.gated #page", css)
        self.assertRegex(css, r"blur\(10px\)")
        page = read("index.html")
        self.assertIn('<div id="page">', page)
        self.assertRegex(page, r'id="gate"[^>]*aria-modal="true"')


class Deployment(unittest.TestCase):
    """The site must work unchanged (a) in the local preview at http://localhost:8000/ and (b) as the project site https://<owner>.github.io/<repo>/ (a sub-path),
    and every link must point at the public stable repository, never at a private one."""

    PUBLIC_DOCS = {"README.md", "CHANGELOG.md"}

    def refs(self):
        out = []
        for name in ("index.html",):
            out += re.findall(r'(?:href|src)="([^"]+)"', read(name))
        css = read("css", "style.css") + read("css", "themes.css")
        out += re.findall(r"url\(\s*[\"']?([^)\"']+)", css)
        return out

    def test_every_path_is_relative_so_the_github_pages_sub_path_and_the_local_preview_both_work(self):
        page = read("index.html")
        self.assertNotRegex(page, r"<base\b")
        generated = {"tsw-forever.flatpakrepo", conf()["APP_ID"] + ".flatpakref"}  # created by the pages-build job of release.yml, next to index.html
        for ref in self.refs():
            if ref.startswith(("http://", "https://", "#", "mailto:")):
                continue
            self.assertFalse(ref.startswith("/"), f"root-absolute path {ref}: it would leave the /TSW-Forever/ sub-path")
            for base in ("http://localhost:8000/", conf()["FLATPAK_REPO_URL"].rsplit("/repo", 1)[0] + "/"):
                url = urllib.parse.urljoin(base, ref)
                self.assertTrue(url.startswith(base), f"{ref} leaves {base}")
            if ref not in generated:
                self.assertTrue(os.path.exists(os.path.join(SITE, ref.split("#")[0])), f"{ref} does not exist in website/")
        for js in ("app.js", "download.js"):
            self.assertNotRegex(read("js", js), r"""(fetch\(|location[.a-z]*\s*=|\.src\s*=|setAttribute\("(href|src)",)\s*["']/""", f"{js}: a root-absolute path")

    def test_the_site_address_is_the_github_pages_address_of_the_public_repository(self):
        cfg = conf()
        site = f"https://{cfg['GITHUB_OWNER'].lower()}.github.io/{cfg['GITHUB_REPO']}"
        self.assertEqual(cfg["FLATPAK_REPO_URL"], site + "/repo")  # the update source never changes its address
        self.assertEqual(cfg["REPO_URL"], f"https://github.com/{cfg['GITHUB_OWNER']}/{cfg['GITHUB_REPO']}")
        page = read("index.html")
        self.assertIn(f"{site}/tsw-forever.flatpakrepo", page)

    def test_every_github_link_points_at_the_public_repository_and_at_a_file_that_the_public_repository_will_have(self):
        sys.path.insert(0, HERE)
        import check_public_source as public
        cfg = conf()
        base = cfg["REPO_URL"]
        readme = open(os.path.join(ROOT, "README.md"), encoding="utf-8").read()
        slugs = {re.sub(r"[^a-z0-9 -]", "", h.lower()).strip().replace(" ", "-") for h in re.findall(r"^#+ (.+)$", readme, re.M)}
        links = [u for u in re.findall(r'href="(https://github\.com[^"]*)"', read("index.html"))]
        self.assertTrue(links)
        for url in links:
            self.assertTrue(url == base or url.startswith(base + "/") or url.startswith(base + "#"), f"not the public repository: {url}")
            m = re.match(re.escape(base) + r"/blob/main/(.+)$", url)
            if m:
                rel = m.group(1)
                ok = rel in public.ROOT_FILES or (rel.startswith("documentation/") and rel[len("documentation/"):] in public.DOCS)
                self.assertTrue(ok and os.path.exists(os.path.join(ROOT, rel)), f"{url}: not part of the public export")
            if "#" in url and url.startswith(base + "#") and url.split("#", 1)[1] != "readme":  # "#readme" is GitHub's own anchor of the rendered README
                self.assertIn(url.split("#", 1)[1], slugs, f"{url}: no such README heading")

    def test_nothing_points_at_a_private_or_testing_artifact(self):
        for dirpath, _, names in os.walk(SITE):
            for n in names:
                with open(os.path.join(dirpath, n), encoding="utf-8") as f:
                    text = f.read()
                self.assertNotRegex(text, r"TSW-Forever-Dev|Forever-Dev|(?i:\btesting\b)|dev-channel|stable-testing", f"{n} mentions a private or testing artifact")


@unittest.skipUnless(shutil.which("bun") or shutil.which("node"), "no JavaScript runtime: the download logic test is skipped")
class DownloadLogic(unittest.TestCase):
    def runtime(self):
        runtime = shutil.which("bun")
        if not runtime and node_works():
            runtime = shutil.which("node")
        if not runtime:
            self.skipTest("no working JavaScript runtime")
        return runtime

    def test_the_update_source_is_shown_only_when_its_files_really_exist(self):
        r = subprocess.run([self.runtime(), os.path.join(HERE, "test_website_source.js")], capture_output=True, text=True, timeout=120)
        self.assertEqual(r.returncode, 0, r.stdout[-1500:] + r.stderr[-800:])
        self.assertGreaterEqual(r.stdout.count('"ok":true'), 14)
        self.assertNotIn('"ok":false', r.stdout)

    def test_the_source_script_asks_only_this_site_and_never_trusts_a_host_name(self):
        js, cfg = read("js", "source.js"), read("js", "config.js")
        self.assertNotIn("github.io", js)
        self.assertNotRegex(js, r"https?://")
        self.assertIn('method: "HEAD"', js)
        self.assertIn('credentials: "same-origin"', js)
        self.assertRegex(cfg, r'updateSourceLive: "auto"')
        probe = re.search(r"sourceProbe: \[([^\]]*)\]", cfg).group(1).replace('"', "").replace(" ", "").split(",")
        self.assertEqual(probe, ["repo/summary", "tsw-forever.flatpakrepo", conf()["APP_ID"] + ".flatpakref"])
        for path in probe:
            self.assertFalse(path.startswith(("/", "http")), path)
        self.assertIn('<script src="js/source.js">', read("index.html"))
    def test_the_download_button_in_every_kind_of_github_answer(self):
        r = subprocess.run([self.runtime(), os.path.join(HERE, "test_website_download.js")], capture_output=True, text=True, timeout=120)
        self.assertEqual(r.returncode, 0, r.stdout[-1500:] + r.stderr[-800:])
        self.assertGreaterEqual(r.stdout.count('"ok":true'), 18)
        self.assertNotIn('"ok":false', r.stdout)


class Editorial(unittest.TestCase):
    """The statements of the editorial review of 2026-11-19: no absolute or unproven claim, the facts of the implementation."""

    @classmethod
    def setUpClass(cls):
        cls.d = {lang: dictionary(lang) for lang in LANGUAGES}

    def test_the_personal_text_has_five_paragraphs_in_every_language(self):
        for lang in LANGUAGES:
            self.assertTrue(all(self.d[lang][f"why.p{i}"].strip() for i in range(1, 6)), lang)
        self.assertTrue(self.d["de"]["why.p1"].startswith("The Secret World ist für mich mehr als nur ein Spiel"))
        self.assertEqual(self.d["de"]["why.p5"], "Nicht, um es neu zu erfinden. Sondern damit das Original bleiben kann, was es ist.")

    def test_no_claim_that_the_implementation_does_not_support(self):
        # absolute or wrong statements removed on purpose (English wording of the old texts and the German ones; the others are checked by key set equality)
        banned = [r"nie ver(ä|a)ndert", r"never changed", r"ohne deinen Klick", r"without your click", r"offen auf GitHub", r"developed in the open", r"einzige Anfrage", r"only request",
                  r"funktioniert immer", r"always works", r"loads nothing from third", r"keinen eigenen Hintergrund-Updater", r"no background updater of its own", r"Bildratenbegrenzung bleiben"]
        for lang, d in self.d.items():
            for key, text in d.items():
                for pat in banned:
                    self.assertNotRegex(text, "(?i)" + pat, f"{lang}:{key}")

    def test_windows_and_linux_features_are_kept_apart(self):
        for lang, d in self.d.items():
            self.assertNotEqual(d["features.settings.title"], "")
        self.assertIn("Linux", self.d["en"]["features.settings.body"])  # the frame-rate limit is named as a Linux feature
        self.assertIn("Linux", self.d["de"]["features.settings.body"])
        for lang, d in self.d.items():
            self.assertIn("Wine/Proton", d["linux.req"], lang)
            self.assertIn("Flathub", d["linux.req"], lang)

    def test_the_faction_names_are_consistent_and_dragon_is_never_translated(self):
        for lang, d in self.d.items():
            self.assertEqual(d["faction.dragon.name"], "Dragon", lang)
            self.assertEqual(d["faction.illuminati.name"], "Illuminati", lang)

    def test_the_placeholders_survive_in_every_language(self):
        for lang, d in self.d.items():
            for ph in ("{version}", "{size}"):
                self.assertIn(ph, d["dl.win.status.found"], lang)

    def test_the_documentation_links_say_that_they_are_english(self):
        for lang in LANGUAGES[1:]:
            for key in ("docs.guide", "docs.readme", "docs.trouble", "docs.build", "docs.changelog"):
                self.assertTrue(self.d[lang][key].endswith("(EN)"), f"{lang}:{key}")

    def test_the_privacy_section_names_github_pages_and_the_release_lookup(self):
        for lang, d in self.d.items():
            self.assertIn("GitHub Pages", d["legal.body3"], lang)
            self.assertIn("GitHub", d["legal.body3"], lang)
            self.assertIn("IP", d["legal.body3"], lang)
        self.assertIn('data-i18n="legal.privacy.title"', read("index.html"))

    def test_the_noscript_note_exists_in_all_six_languages(self):
        m = re.search(r"<noscript>(.*?)</noscript>", read("index.html"), re.S)
        self.assertEqual(re.findall(r'<p lang="([^"]+)">', m.group(1)), LANGUAGES)

    def test_the_flatpakref_is_the_primary_graphical_way(self):
        page = read("index.html")
        self.assertRegex(page, r'class="btn primary" href="' + re.escape(conf()["APP_ID"]) + r'\.flatpakref"')
        self.assertRegex(page, r'class="btn" href="tsw-forever\.flatpakrepo"')

    def test_the_manual_update_command_has_no_placeholder(self):
        for c in re.findall(r"<pre><code>(.*?)</code></pre>", read("index.html"), re.S):
            self.assertNotIn("&lt;version&gt;", c)
            self.assertNotIn("<version>", html.unescape(c))


class PagesAssembly(unittest.TestCase):
    def test_the_pages_assembler_accepts_the_site_next_to_a_signed_repository_layout(self):
        d = tempfile.mkdtemp(prefix="tsw-site-")
        try:
            app = pages.APP_ID
            commit = "a" * 64

            def w(path, text):
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w") as f:
                    f.write(text)

            w(os.path.join(d, "repo", "summary"), "s")
            w(os.path.join(d, "repo", "objects", "ab", "cd.file"), "o")
            w(os.path.join(d, "repo", "refs", "heads", "app", app, "x86_64", "stable"), commit + "\n")
            w(os.path.join(d, "refs", "tsw-forever.flatpakrepo"), "[Flatpak Repo]")
            w(os.path.join(d, "refs", f"{app}.flatpakref"), "[Flatpak Ref]")
            out = os.path.join(d, "site")
            self.assertGreater(pages.assemble(os.path.join(d, "repo"), os.path.join(d, "refs"), out, website=SITE), 0)
            self.assertTrue(os.path.isfile(os.path.join(out, "index.html")) and os.path.isfile(os.path.join(out, "repo", "summary")))
            self.assertTrue(os.path.isfile(os.path.join(out, "css", "themes.css")))
        finally:
            shutil.rmtree(d, ignore_errors=True)


def node_works():
    """node must exist AND start (a node with a missing library is as good as none)."""
    try:
        return shutil.which("node") is not None and subprocess.run(["node", "--version"], capture_output=True).returncode == 0
    except OSError:
        return False


@unittest.skipUnless(node_works(), "node is not available here: the JavaScript syntax check is skipped")
class Syntax(unittest.TestCase):
    def test_every_script_parses(self):
        for dirpath, _, names in os.walk(os.path.join(SITE, "js")):
            for n in names:
                r = subprocess.run(["node", "--check", os.path.join(dirpath, n)], capture_output=True, text=True)
                self.assertEqual(r.returncode, 0, f"{n}: {r.stderr}")


if __name__ == "__main__":
    unittest.main()
