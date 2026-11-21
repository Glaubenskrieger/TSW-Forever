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
SCRIPT_KEYS = {"msg.storage", "ui.copied", "ui.copyfail", "dl.status.found", "dl.status.failed", "dl.status.none"}
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
                if n != "download.js":
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
        pages.check_website(SITE)  # no installation file, no Flatpak repository or install file, no release file, no link, nothing but plain web content
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

    def test_the_fallback_button_is_the_release_list_of_the_project_and_the_archive_link_is_visible(self):
        page = read("index.html")
        cfg = conf()
        for platform in ("windows", "linux"):
            links = re.findall(r'class="btn(?: primary)? js-dl-' + platform + r'" href="([^"]+)"', page)
            self.assertEqual(len(links), 2, f"{platform}: one button in the hero and one in its download card")
            self.assertEqual(set(links), {cfg["REPO_URL"] + "/releases"}, "without JavaScript a button opens the list of releases, never a file that might not exist")
        self.assertRegex(page, r'<a class="all-releases" href="' + re.escape(cfg["REPO_URL"] + "/releases") + '"')
        self.assertIn('id="download-linux"', page)
        self.assertIn('id="download-windows"', page)  # the anchors the launcher opens

    def test_the_flatpak_commands_install_a_bundle_and_there_is_no_update_source_any_more(self):
        page = read("index.html")
        cfg = conf()
        commands = [html.unescape(c) for c in re.findall(r"<pre><code>(.*?)</code></pre>", page, re.S)]
        self.assertIn("flatpak install --user ./tsw-forever-*.flatpak", commands)
        self.assertIn(f"flatpak run {cfg['APP_ID']}", commands)
        self.assertIn("sha256sum tsw-forever-*.flatpak", commands)
        for gone in ("flatpakref", "flatpakrepo", "remote-add", "repo/summary", "update-source", "data-update-source", "source.js"):
            self.assertNotIn(gone, page, f"the page must not mention {gone}: no Flatpak repository is hosted")
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
        for ref in self.refs():
            if ref.startswith(("http://", "https://", "#", "mailto:")):
                continue
            self.assertFalse(ref.startswith("/"), f"root-absolute path {ref}: it would leave the /TSW-Forever/ sub-path")
            for base in ("http://localhost:8000/", conf()["WEBSITE_URL"]):
                url = urllib.parse.urljoin(base, ref)
                self.assertTrue(url.startswith(base), f"{ref} leaves {base}")
            self.assertTrue(os.path.exists(os.path.join(SITE, ref.split("#")[0])), f"{ref} does not exist in website/")
        for js in ("app.js", "download.js"):
            self.assertNotRegex(read("js", js), r"""(fetch\(|location[.a-z]*\s*=|\.src\s*=|setAttribute\("(href|src)",)\s*["']/""", f"{js}: a root-absolute path")

    def test_the_site_address_is_the_github_pages_address_of_the_public_repository(self):
        cfg = conf()
        site = f"https://{cfg['GITHUB_OWNER'].lower()}.github.io/{cfg['GITHUB_REPO']}"
        self.assertEqual(cfg["WEBSITE_URL"], site + "/")  # the address that the launcher opens never changes
        self.assertEqual(cfg["REPO_URL"], f"https://github.com/{cfg['GITHUB_OWNER']}/{cfg['GITHUB_REPO']}")
        self.assertNotIn("flatpakrepo", read("index.html"))

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

    def test_the_download_script_asks_only_github_for_the_release_metadata_and_names_no_version(self):
        js, cfg = read("js", "download.js"), read("js", "config.js")
        self.assertEqual(sorted(set(re.findall(r"https?://[^\"' ]+", js))), ["https://api.github.com/repos/", "https://github.com/"], "only GitHub's API and the release downloads of the repository")
        self.assertIn("/releases/latest", js)  # GitHub's latest is never a draft or a pre-release
        self.assertIn("draft", js)
        self.assertIn("prerelease", js)
        self.assertIn('credentials: "omit"', js)
        self.assertIn('referrerPolicy: "no-referrer"', js)
        self.assertNotRegex(js + cfg, r"\b\d+\.\d+\.\d+\b", "no version number is written into the scripts")
        self.assertNotIn("updateSourceLive", cfg)
        self.assertNotIn("sourceProbe", cfg)
        self.assertFalse(os.path.exists(os.path.join(SITE, "js", "source.js")), "the probe of the former Flatpak update source is gone")

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

    def test_the_personal_text_has_four_parts_in_every_language(self):
        # lead line, two paragraphs, the guiding sentence (revision of 2026-11-21); the three cards below it are separate keys and unchanged
        for lang in LANGUAGES:
            self.assertTrue(all(self.d[lang][f"why.p{i}"].strip() for i in range(1, 5)), lang)
            self.assertNotIn("why.p5", self.d[lang], f"{lang}: the old fifth paragraph is gone")
            for key in ("why.preserve.title", "why.preserve.body", "why.systems.title", "why.systems.body", "why.legends.title", "why.legends.body"):
                self.assertTrue(self.d[lang][key].strip(), f"{lang}:{key}")
        self.assertEqual(self.d["de"]["why.p1"], "Manche Welten lassen einen niemals los.")
        self.assertEqual(self.d["de"]["why.p2"], "The Secret World ist für mich eine davon. Seit 2012 begleiten mich seine Geheimnisse, seine Geschichten und diese unverwechselbare Atmosphäre.")
        self.assertEqual(self.d["de"]["why.p3"], "TSW-Forever ist mein Beitrag dazu, dass diese Welt auch auf modernen Systemen weiterlebt – ohne das Original zu verändern.")
        self.assertEqual(self.d["de"]["why.p4"], "Nicht, um es neu zu erfinden. Sondern damit das Original bleiben kann, was es ist.")

    def test_the_guiding_sentence_is_a_quote_with_a_thin_dark_red_rule_and_no_box(self):
        css = read("css", "style.css")
        rule = re.search(r"\.story \.closing \{([^}]*)\}", css).group(1)
        self.assertRegex(rule, r"font-style:\s*italic")
        self.assertRegex(rule, r"border-left:\s*2px solid var\(--accent\)")
        self.assertRegex(rule, r"padding-left:")
        self.assertNotRegex(rule, r"background|box-shadow|border:|border-radius")
        self.assertRegex(read("index.html"), r'<p class="closing" data-i18n="why\.p4">')

    def test_there_is_no_link_to_the_issue_tracker_and_no_text_that_asks_for_one(self):
        # the project's GitHub issue tracker is switched off: nothing on the site may point at it or invite a report through it
        for rel in ("index.html", os.path.join("js", "app.js"), os.path.join("js", "config.js"), os.path.join("js", "download.js")):
            self.assertNotRegex(read(*rel.split(os.sep)).lower(), r"/issues|issue tracker|github issues", rel)
        for lang in LANGUAGES:
            self.assertNotIn("docs.issues", self.d[lang], lang)
            for key, text in self.d[lang].items():
                self.assertNotRegex(text.lower(), r"/issues|issue tracker|github issues|issue-tracker", f"{lang}:{key}")
        # the other GitHub links stay (documentation, source, releases)
        page = read("index.html")
        for keep in ("/releases", "user-guide.md", "#readme", "CHANGELOG.md"):
            self.assertIn(keep, page)

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
                self.assertIn(ph, d["dl.status.found"], lang)

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

    def test_both_downloads_are_primary_buttons_and_the_archive_link_stays_visible(self):
        page = read("index.html")
        self.assertRegex(page, r'<a class="btn primary js-dl-linux"')
        self.assertRegex(page, r'<a class="btn primary js-dl-windows"')
        self.assertIn('class="all-releases"', page)

    def test_the_manual_update_command_has_no_placeholder(self):
        for c in re.findall(r"<pre><code>(.*?)</code></pre>", read("index.html"), re.S):
            self.assertNotIn("&lt;version&gt;", c)
            self.assertNotIn("<version>", html.unescape(c))


class UpdateInstructions(unittest.TestCase):
    """0.4.1: the launcher CHECKS for a newer version and shows a notice; it never downloads or installs anything. The page explains the manual update for Windows and for Linux/Flatpak
    separately, in plain words, in all six languages, and must never promise an automatic update."""

    UPDATE_KEYS = ["dl.update.hint", "updates.intro", "updates.manual", "updates.win.lead", "updates.win.s1", "updates.win.s2", "updates.win.s3", "updates.win.s4",
                   "updates.linux.lead", "updates.linux.s1", "updates.linux.s2", "updates.linux.s3", "updates.linux.s4", "updates.linux.warn", "updates.faq.title"] + \
                  [f"updates.{k}{i}" for i in range(1, 6) for k in ("q", "a")]
    # claims of an automatic update or of an updater inside the program, per language (the sentences that DENY it use other words: "by itself", "von allein", "sozinho", ...)
    AUTOMATIC = {
        "en": r"automatic|auto-?updat|in the background|updates itself|installs? (it |them )?for you",
        "de": r"automatisch|im Hintergrund|aktualisiert sich selbst|installiert (es |sie )?für dich",
        "fr": r"automatique|en arrière-plan|se met à jour (tout seul|lui-même)|l'installe pour vous",
        "es": r"automátic|segundo plano|se actualiza (solo|por sí)|lo instala por ti",
        "pt-BR": r"automátic|segundo plano|se atualiza sozinho|instala (isso )?para você",
        "pl": r"automatyczn|w tle|aktualizuje się sam|zainstaluje (go )?za ciebie",
    }

    @classmethod
    def setUpClass(cls):
        cls.d = {lang: dictionary(lang) for lang in LANGUAGES}
        cls.page = read("index.html")

    def test_every_language_has_the_complete_update_instructions(self):
        for lang in LANGUAGES:
            for key in self.UPDATE_KEYS:
                self.assertTrue(self.d[lang].get(key, "").strip(), f"{lang}:{key}")
            self.assertEqual(len({self.d[lang][f"updates.win.s{i}"] for i in range(1, 5)}), 4, lang)
            self.assertNotIn("updates.win.body", self.d[lang], f"{lang}: the old one-paragraph text is replaced by the steps")
            self.assertNotIn("updates.linux.body", self.d[lang], lang)

    def test_windows_and_linux_are_explained_separately_with_their_own_steps_and_the_linux_command(self):
        page = self.page
        for needle in ('id="updates"', 'id="update-windows"', 'id="update-linux"', 'href="#updates"'):
            self.assertIn(needle, page)
        win = page[page.index('id="update-windows"'):page.index('id="update-linux"')]
        linux = page[page.index('id="update-linux"'):page.index('class="faq"')]
        self.assertEqual(len(re.findall(r"<li data-i18n=\"updates\.win\.s\d\"", win)), 4)
        self.assertEqual(len(re.findall(r"<li data-i18n=\"updates\.linux\.s\d\"", linux)), 4)
        self.assertNotIn("flatpak", win.lower(), "the Windows steps do not mention Flatpak")
        self.assertNotRegex(linux, r"(?i)setup\.exe|PowerShell|Start menu")
        self.assertIn("flatpak install --user ./tsw-forever-X.Y.Z.flatpak", html.unescape(linux))
        self.assertIn("updates.linux.warn", linux, "the warning about installing an older bundle is part of the Linux steps")
        self.assertEqual(len(re.findall(r"<details>", page)), 5)
        for i in range(1, 6):
            self.assertIn(f'<summary data-i18n="updates.q{i}">', page)
            self.assertIn(f'data-i18n="updates.a{i}"', page)

    def test_the_steps_name_the_buttons_the_user_really_sees_in_the_launcher_and_on_the_page(self):
        # the launcher's own button text (its translation catalogue) is quoted in the steps; the page's own button texts as well
        for lang in LANGUAGES:
            label = "Open download page" if lang == "en" else None
            if lang != "en":
                po = open(os.path.join(ROOT, "launcher-gui", "translations", lang, "LC_MESSAGES", "tsw-launcher-gui.po"), encoding="utf-8").read()
                m = re.search(r'msgid "Open download page"\nmsgstr "([^"]+)"', po)
                self.assertIsNotNone(m, lang)
                label = m.group(1)
            for key in ("updates.win.s1", "updates.linux.s1"):
                self.assertIn(label, self.d[lang][key], f"{lang}:{key} must quote the launcher button {label!r}")
            self.assertIn(self.d[lang]["dl.win.button"], self.d[lang]["updates.win.s2"], lang)
            self.assertIn(self.d[lang]["dl.linux.button"], self.d[lang]["updates.linux.s2"], lang)

    def test_no_language_promises_an_automatic_update_or_a_download_by_the_launcher(self):
        for lang in LANGUAGES:
            for key in self.UPDATE_KEYS:
                self.assertNotRegex(self.d[lang][key], "(?i)" + self.AUTOMATIC[lang], f"{lang}:{key}")
        # the English page text without JavaScript is the English dictionary text: the whole page is free of such claims, apart from the sentences that deny it
        visible = re.sub(r"<[^>]+>", " ", re.sub(r"(?s)<(script|style).*?</\1>", " ", self.page))
        for sentence in re.split(r"(?<=[.!?])\s+", html.unescape(visible)):
            # only sentences about the launcher, updates and installation (the noscript note speaks of the web site's own download search)
            if re.search(r"(?i)launcher|update|install", sentence) and re.search(self.AUTOMATIC["en"], sentence, re.I):
                self.assertRegex(sentence, r"(?i)\b(never|not|no|nothing|without)\b", sentence)

    def test_the_texts_say_that_the_user_decides_and_that_nothing_is_installed_by_the_launcher(self):
        en = self.d["en"]
        self.assertRegex(en["updates.intro"], r"never downloads or installs anything by itself")
        self.assertRegex(en["updates.intro"], r"You decide whether and when")
        self.assertRegex(en["updates.manual"], r"by hand")
        self.assertRegex(en["updates.a2"], r"^No\.")
        self.assertRegex(en["updates.a3"], r"^No\.")
        self.assertRegex(en["updates.linux.warn"], r"older")
        for lang in LANGUAGES:
            self.assertNotEqual(self.d[lang]["updates.intro"], en["updates.intro"] if lang != "en" else None, lang)


class DownloadCardsAlignment(unittest.TestCase):
    """The two download buttons start at the same height on wide screens (shared grid rows), without touching any other card."""

    def test_both_cards_have_the_same_five_parts_in_the_same_order(self):
        page = read("index.html")
        grid = page[page.index('class="grid two dl-grid"'):page.index('class="all-releases"')]
        cards = re.findall(r'<div class="card dl" id="(download-[a-z]+)">(.*?)\n        </div>', grid, re.S)
        self.assertEqual([c[0] for c in cards], ["download-linux", "download-windows"])
        shapes = []
        for _id, body in cards:
            tags = re.findall(r"<(h3|p)([^>]*)>", body)
            self.assertEqual(len(tags), 5, _id)
            self.assertEqual(tags[0][0], "h3")
            self.assertIn("js-dl-", body.split("</p>")[1], "the button is the third part (after the title and the description)")
            self.assertIn("role=\"status\"", tags[3][1], "the status text is the fourth part")
            self.assertIn("hidden", tags[4][1], "the checksum is the fifth part")
            shapes.append([t[0] for t in tags])
        self.assertEqual(shapes[0], shapes[1])

    def test_the_css_shares_the_rows_of_the_two_cards_without_a_fixed_breakpoint(self):
        css = re.sub(r"/\*.*?\*/", "", read("css", "style.css"), flags=re.S)  # rules only, not comments
        block = css[css.index("@supports (grid-template-rows: subgrid)"):]
        self.assertIn(".dl-grid > .card.dl { display: grid; grid-row: span 5; grid-template-rows: subgrid; row-gap: 0; }", block)
        for i in range(1, 6):
            self.assertIn(f".dl-grid > .card.dl > :nth-child({i}) {{ grid-row: {i}; }}", block)
        # the grid keeps its normal gap (cards below each other stay apart), and no viewport breakpoint decides whether the rows are shared
        self.assertNotRegex(css, r"\.dl-grid\s*\{[^}]*row-gap")
        self.assertNotRegex(css[css.index(".dl-grid > .card.dl {") - 200:css.index(".dl-grid > .card.dl {")], r"@media")
        self.assertNotRegex(block.split("@container")[0], r"height\s*:|min-height|\bpx\b")

    def test_the_other_card_grids_are_untouched(self):
        page = read("index.html")
        self.assertEqual(page.count("dl-grid"), 1)
        self.assertEqual(page.count('<div class="grid two">'), 1, "the update cards keep the plain grid")


class HarmonisedLayout(unittest.TestCase):
    """The layout values are defined once, the text has one measure, and the two layout bugs found on 2026-11-21 stay fixed."""

    @classmethod
    def setUpClass(cls):
        cls.css = re.sub(r"/\*.*?\*/", "", read("css", "style.css"), flags=re.S)
        cls.page = read("index.html")

    def test_one_page_column_and_one_text_measure(self):
        self.assertIn("--content-width: 1080px;", self.css)
        self.assertIn("--content-measure: 46rem;", self.css)
        self.assertIn(".wrap { width: min(var(--content-width), 100% - 2rem); margin-inline: auto; }", self.css)
        self.assertIn("p { margin: 0 0 var(--space-para); color: var(--dim); max-width: var(--content-measure); }", self.css)
        # hero, footer and dialog keep their own text widths
        for own in (".hero p {", "footer p {", ".gate-box p {"):
            self.assertIn("max-width: 68ch", self.css[self.css.index(own):self.css.index("}", self.css.index(own))], own)

    def test_the_vertical_distances_come_from_one_small_scale(self):
        for token in ("--space-card-gap: 1.1rem", "--space-heading: 1.3rem", "--space-para: 1rem", "--space-intro: 1.5rem", "--space-story: 2.2rem"):
            self.assertIn(token, self.css)
        self.assertIn("section h2 { margin-bottom: var(--space-heading); }", self.css)

    def test_the_two_lead_styles_have_their_own_names_and_the_story_text_is_not_an_island_any_more(self):
        self.assertNotRegex(self.css, r"(?<![\w-])\.lead\b")
        self.assertNotIn('class="lead"', self.page)
        self.assertIn('class="hero-lead"', self.page)
        self.assertIn('class="story-lead"', self.page)
        self.assertNotRegex(self.css, r"\.story p \{[^}]*(font-size|line-height|max-width: 86ch)")

    def test_the_sha_256_values_can_break_inside_their_cards(self):
        self.assertIn(".dl code { overflow-wrap: anywhere; word-break: break-all; }", self.css)
        self.assertNotIn("#win-sha", self.css, "the old id no longer exists")
        for sid in ("linux-sha", "windows-sha"):
            self.assertIn(f'id="{sid}"', self.page)

    def test_both_installation_cards_share_the_wide_card_width(self):
        self.assertEqual(self.page.count('<div class="card wide">'), 2)
        self.assertIn(".card.wide { max-width: calc(var(--content-measure) + 2 * var(--card-pad-x) + 2px); }", self.css)

    def test_the_grids_of_three_are_one_column_or_three_never_two_and_one(self):
        self.assertEqual(self.page.count('<div class="grid three">'), 2)
        self.assertIn(".grid.three { grid-template-columns: 1fr; }", self.css)
        self.assertIn("@container (min-width: 50.2rem) {\n  .grid.three { grid-template-columns: repeat(3, minmax(0, 1fr)); }", self.css)
        self.assertGreaterEqual(50.2, 3 * 16 + 2 * 1.1)

    def test_the_hero_buttons_are_equal_and_stacked_on_narrow_screens(self):
        self.assertIn(".hero .actions { flex-direction: column; align-items: center; }", self.css)
        self.assertIn(".hero .actions .btn { width: min(100%, 24rem); }", self.css)


class StagingPreview(unittest.TestCase):
    """The site that CI keeps (artifact tsw-forever-site-<commit>) is looked at locally under the production path /TSW-Forever/ by scripts/website_preview.sh --from-artifact."""

    SCRIPT = os.path.join(HERE, "website_preview.sh")

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.art = os.path.join(self.tmp, "artifact")
        pages.assemble(SITE, os.path.join(self.art, "site"))
        lines = []
        for d, _, files in sorted(os.walk(os.path.join(self.art, "site"))):
            for n in sorted(files):
                full = os.path.join(d, n)
                import hashlib
                lines.append(f"{hashlib.sha256(open(full, 'rb').read()).hexdigest()}  ./{os.path.relpath(full, os.path.join(self.art, 'site'))}")
        open(os.path.join(self.art, "SHA256SUMS-site"), "w").write("\n".join(lines) + "\n")
        open(os.path.join(self.art, "COMMIT"), "w").write("0123456789abcdef0123456789abcdef01234567\n")
        self.build = os.path.join(self.tmp, "build")

    def tearDown(self):
        shutil.rmtree(self.tmp)

    def run_preview(self, *args):
        env = dict(os.environ, TSW_PREVIEW_BUILD=self.build, TSW_PREVIEW_PREPARE_ONLY="1")
        return subprocess.run(["bash", self.SCRIPT, *args], capture_output=True, text=True, env=env, timeout=60)

    def test_a_downloaded_artifact_folder_is_served_under_the_production_path(self):
        r = self.run_preview("--from-artifact", self.art, "8123")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertIn("checksums of the artifact: ok", r.stdout)
        self.assertIn("built from commit: 0123456789abcdef0123456789abcdef01234567", r.stdout)
        self.assertIn("http://127.0.0.1:8123/TSW-Forever/", r.stdout)
        served = os.path.join(self.build, "site-preview", "TSW-Forever")
        self.assertTrue(os.path.isfile(os.path.join(served, "index.html")))
        self.assertTrue(os.path.isfile(os.path.join(served, ".nojekyll")))
        # byte-identical to the artifact
        for d, _, files in os.walk(os.path.join(self.art, "site")):
            for n in files:
                rel = os.path.relpath(os.path.join(d, n), os.path.join(self.art, "site"))
                self.assertEqual(open(os.path.join(d, n), "rb").read(), open(os.path.join(served, rel), "rb").read(), rel)

    def test_a_zip_as_downloaded_from_github_works_too(self):
        zpath = os.path.join(self.tmp, "tsw-forever-site.zip")
        shutil.make_archive(zpath[:-4], "zip", self.art)
        r = self.run_preview("--from-artifact", zpath)
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertTrue(os.path.isfile(os.path.join(self.build, "site-preview", "TSW-Forever", "index.html")))

    def test_a_changed_file_a_forbidden_file_and_a_missing_site_are_refused(self):
        with open(os.path.join(self.art, "site", "js", "app.js"), "a") as f:
            f.write("// tampered\n")
        r = self.run_preview("--from-artifact", self.art)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)
        self.assertIn("do not match", r.stderr)
        self.assertFalse(os.path.exists(os.path.join(self.build, "site-preview", "TSW-Forever")))
        shutil.rmtree(self.art)
        os.makedirs(self.art)
        open(os.path.join(self.art, "readme.txt"), "w").write("x")
        r = self.run_preview("--from-artifact", self.art)
        self.assertEqual(r.returncode, 1)
        self.assertIn("no site", r.stderr)
        self.assertEqual(self.run_preview("--from-artifact", os.path.join(self.tmp, "nothing")).returncode, 1)

    def test_an_installer_inside_the_artifact_is_refused_by_the_assemblers_rules(self):
        open(os.path.join(self.art, "site", "setup.exe"), "w").write("x")
        os.remove(os.path.join(self.art, "SHA256SUMS-site"))
        r = self.run_preview("--from-artifact", self.art)
        self.assertEqual(r.returncode, 1, r.stdout + r.stderr)

    def test_the_existing_modes_still_work_and_the_arguments_are_checked(self):
        r = self.run_preview("--as-pages", "8124")
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertTrue(os.path.isfile(os.path.join(self.build, "site-preview", "TSW-Forever", "index.html")))
        self.assertIn("http://127.0.0.1:8124/TSW-Forever/", r.stdout)
        self.assertEqual(self.run_preview("8125").returncode, 0)
        self.assertEqual(self.run_preview("--bogus").returncode, 2)
        self.assertEqual(self.run_preview("--from-artifact").returncode, 2)

    def test_the_script_serves_on_the_loopback_address_only(self):
        text = open(self.SCRIPT, encoding="utf-8").read()
        self.assertIn("--bind 127.0.0.1", text)
        self.assertNotIn("0.0.0.0", text)


class PagesAssembly(unittest.TestCase):
    def test_the_pages_artifact_is_the_web_site_only_and_small(self):
        d = tempfile.mkdtemp(prefix="tsw-site-")
        try:
            out = os.path.join(d, "site")
            total = pages.assemble(SITE, out)
            self.assertLess(total, 512 * 1024)
            names = {os.path.relpath(os.path.join(dp, n), out) for dp, _, ns in os.walk(out) for n in ns}
            self.assertIn("index.html", names)
            self.assertFalse([n for n in names if n.endswith((".flatpak", ".flatpakref", ".flatpakrepo", ".exe", ".gz", ".gpg", ".filez")) or n.startswith(("repo", "channels", "site-manifest"))], names)
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
