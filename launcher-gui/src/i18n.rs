//! Localization of the launcher window.
//!
//! One source of truth: gettext `.po` files under `translations/<lang>/LC_MESSAGES/tsw-launcher-gui.po`. The message ids are
//! the English texts, so English is the source language and needs no file.
//!
//! * Texts in the `.slint` file use `@tr("…")`; `build.rs` bundles the `.po` files into the program and
//!   [`select_language`] switches them at run time (no restart).
//! * Texts made by the program (status line, checks, errors, progress) are [`Msg`] values: an id plus arguments. They are
//!   rendered through [`Catalog`], which reads the same `.po` files, so a translator edits one file per language.
//! * A message that has no translation falls back to the English id; the id is never replaced by a different language.
//!
//! Add a language: create `translations/<code>/LC_MESSAGES/tsw-launcher-gui.po` and add one line to [`LANGUAGES`]. The tests
//! then require every message id to be translated (or fail with the list of missing ids).

use std::collections::HashMap;

/// `(code, name of the language in that language, .po text)`. English is the source language and has no catalog.
pub const LANGUAGES: &[(&str, &str, &str)] = &[("en", "English", ""), ("de", "Deutsch", include_str!("../translations/de/LC_MESSAGES/tsw-launcher-gui.po"))];

/// The source language, used whenever a translation is missing.
pub const FALLBACK: &str = "en";

/// One value inserted into a message: shown as it is, or itself a message id that is translated first.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Arg {
    Text(String),
    Id(String),
    /// A whole message inside a message (rendered with the same catalog).
    Msg(Box<Msg>),
}

impl From<String> for Arg {
    fn from(s: String) -> Arg {
        Arg::Text(s)
    }
}
impl From<&str> for Arg {
    fn from(s: &str) -> Arg {
        Arg::Text(s.to_string())
    }
}
impl From<&String> for Arg {
    fn from(s: &String) -> Arg {
        Arg::Text(s.clone())
    }
}

impl From<Msg> for Arg {
    fn from(m: Msg) -> Arg {
        Arg::Msg(Box::new(m))
    }
}

/// A user-visible message: the English text as id, plus the values for `{0}`, `{1}`, … in it.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Msg {
    pub id: String,
    pub args: Vec<Arg>,
}

impl Msg {
    pub fn new<A: Into<Arg>>(id: &str, args: impl IntoIterator<Item = A>) -> Msg {
        Msg { id: id.to_string(), args: args.into_iter().map(Into::into).collect() }
    }
    /// A message without arguments.
    pub fn plain(id: &str) -> Msg {
        Msg::new(id, Vec::<String>::new())
    }
    /// Text that is shown as it is (technical detail, a path, a message from a library): never translated.
    pub fn raw(text: impl Into<String>) -> Msg {
        Msg { id: text.into(), args: Vec::new() }
    }
}

/// Parse the message ids and translations of a `.po` file. Empty translations and the header entry are skipped.
pub fn parse_po(text: &str) -> HashMap<String, String> {
    fn unquote(s: &str) -> String {
        let s = s.trim();
        let inner = s.strip_prefix('"').and_then(|t| t.strip_suffix('"')).unwrap_or("");
        let mut out = String::new();
        let mut chars = inner.chars();
        while let Some(c) = chars.next() {
            if c != '\\' {
                out.push(c);
                continue;
            }
            match chars.next() {
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('"') => out.push('"'),
                Some('\\') => out.push('\\'),
                Some(o) => {
                    out.push('\\');
                    out.push(o);
                }
                None => out.push('\\'),
            }
        }
        out
    }
    #[derive(PartialEq)]
    enum In {
        None,
        Id,
        Str,
    }
    let (mut map, mut id, mut tr, mut state) = (HashMap::new(), String::new(), String::new(), In::None);
    let mut flush = |id: &mut String, tr: &mut String| {
        if !id.is_empty() && !tr.is_empty() {
            map.insert(std::mem::take(id), std::mem::take(tr));
        }
        id.clear();
        tr.clear();
    };
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix("msgid ") {
            flush(&mut id, &mut tr);
            id = unquote(rest);
            state = In::Id;
        } else if let Some(rest) = line.strip_prefix("msgstr ") {
            tr = unquote(rest);
            state = In::Str;
        } else if line.starts_with('"') {
            match state {
                In::Id => id.push_str(&unquote(line)),
                In::Str => tr.push_str(&unquote(line)),
                In::None => {}
            }
        } else if line.is_empty() {
            flush(&mut id, &mut tr);
            state = In::None;
        }
    }
    flush(&mut id, &mut tr);
    map
}

/// Insert `{0}`, `{1}`, … Unknown placeholders stay visible instead of being dropped.
pub fn fill(template: &str, args: &[String]) -> String {
    let mut out = template.to_string();
    for (i, a) in args.iter().enumerate() {
        out = out.replace(&format!("{{{i}}}"), a);
    }
    out
}

/// Translations of one language.
#[derive(Clone, Debug)]
pub struct Catalog {
    code: &'static str,
    table: HashMap<String, String>,
}

impl Catalog {
    /// The catalog for `code` (a code from [`LANGUAGES`]); an unknown code gives the English source language.
    pub fn new(code: &str) -> Catalog {
        match LANGUAGES.iter().find(|(c, _, _)| *c == code) {
            Some((c, _, po)) => Catalog { code: c, table: parse_po(po) },
            None => Catalog { code: FALLBACK, table: HashMap::new() },
        }
    }
    pub fn code(&self) -> &'static str {
        self.code
    }
    /// Translate an id; the English id itself when there is no translation.
    pub fn tr<'a>(&'a self, id: &'a str) -> &'a str {
        self.table.get(id).map(String::as_str).unwrap_or(id)
    }
    pub fn render(&self, m: &Msg) -> String {
        let args: Vec<String> = m
            .args
            .iter()
            .map(|a| match a {
                Arg::Text(t) => t.clone(),
                Arg::Id(id) => self.tr(id).to_string(),
                Arg::Msg(inner) => self.render(inner),
            })
            .collect();
        fill(self.tr(&m.id), &args)
    }
    /// Does this language have a translation for `id` (English always counts as having one)?
    #[cfg(test)]
    pub fn has(&self, id: &str) -> bool {
        self.code == FALLBACK || self.table.contains_key(id)
    }

    /// Decimal number with the language's separator: `41,2` (de) or `41.2` (en).
    pub fn decimal(&self, value: f64) -> String {
        let s = format!("{value:.1}");
        if self.code == "de" {
            s.replace('.', ",")
        } else {
            s
        }
    }
    /// `63 %` (de) or `63%` (en).
    pub fn percent(&self, p: u32) -> String {
        if self.code == "de" {
            format!("{p} %")
        } else {
            format!("{p}%")
        }
    }
    /// Decimal units: `41,2 MB`.
    pub fn bytes(&self, n: u64) -> String {
        const U: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
        let (mut v, mut i) = (n as f64, 0);
        while v >= 1000.0 && i < U.len() - 1 {
            v /= 1000.0;
            i += 1;
        }
        if i == 0 {
            format!("{n} B")
        } else {
            format!("{} {}", self.decimal(v), U[i])
        }
    }
    /// `3 s`, `1 min 5 s`, `2 h 4 min`.
    pub fn duration(&self, secs: u64) -> String {
        match secs {
            0..=59 => format!("{secs} s"),
            60..=3599 => format!("{} min {} s", secs / 60, secs % 60),
            _ => format!("{} h {} min", secs / 3600, secs % 3600 / 60),
        }
    }
}

/// The language code to use: the explicit setting if it is available, else the system locale, else English.
/// `setting` is `None`/`"auto"` for "follow the system"; `env` looks up an environment variable.
pub fn resolve(setting: Option<&str>, env: &dyn Fn(&str) -> Option<String>) -> &'static str {
    match setting.map(str::trim) {
        Some(s) if !s.is_empty() && s != "auto" => supported(s).unwrap_or(FALLBACK),
        _ => system_language(env),
    }
}

/// The language of the system: on Windows the display language (asked from Windows; a Windows program has no `LANG`), elsewhere the locale variables.
/// An unsupported language gives the fallback (English).
fn system_language(env: &dyn Fn(&str) -> Option<String>) -> &'static str {
    // an explicitly set locale variable (e.g. in a shell) wins; a normal Windows program has none and follows the display language
    #[cfg(windows)]
    if ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"].iter().all(|v| env(v).is_none_or(|s| s.is_empty())) {
        if let Some(tag) = windows_display_language() {
            return supported(&tag).unwrap_or(FALLBACK);
        }
    }
    from_locale(env)
}

/// The Windows display language as a locale name (`de-DE`), from `GetUserDefaultUILanguage` and `LCIDToLocaleName`.
#[cfg(windows)]
fn windows_display_language() -> Option<String> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetUserDefaultUILanguage() -> u16;
        fn LCIDToLocaleName(locale: u32, name: *mut u16, len: i32, flags: u32) -> i32;
    }
    let mut buf = [0u16; 85]; // LOCALE_NAME_MAX_LENGTH
                              // SAFETY: `buf` is a writable buffer of the length passed; Windows writes a NUL-terminated name into it and returns the number of characters (0 = error).
    let n = unsafe {
        let id = GetUserDefaultUILanguage();
        LCIDToLocaleName(u32::from(id), buf.as_mut_ptr(), buf.len() as i32, 0)
    };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..(n as usize - 1)]))
}

/// `de_DE.UTF-8` / `de-AT` / `de` → `de`, if that language is available.
fn supported(tag: &str) -> Option<&'static str> {
    let lang = tag.split(['.', '@']).next().unwrap_or("").split(['_', '-']).next().unwrap_or("").to_ascii_lowercase();
    LANGUAGES.iter().find(|(c, _, _)| *c == lang).map(|(c, _, _)| *c)
}

/// The first supported language of `LC_ALL`, `LC_MESSAGES`, `LANGUAGE` (colon list) and `LANG`, in that order.
pub fn from_locale(env: &dyn Fn(&str) -> Option<String>) -> &'static str {
    for var in ["LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        let Some(value) = env(var).filter(|v| !v.is_empty()) else {
            continue;
        };
        for tag in value.split(':') {
            if tag == "C" || tag == "POSIX" {
                return FALLBACK;
            }
            if let Some(code) = supported(tag) {
                return code;
            }
        }
    }
    FALLBACK
}

/// Make Slint's `@tr()` texts use `code` (bundled translations). English selects the source texts.
pub fn select_language(code: &str) {
    let _ = slint::select_bundled_translation(if code == FALLBACK { "" } else { code });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string())
    }

    #[test]
    fn po_files_are_parsed_with_escapes_and_multi_line_strings() {
        let m = parse_po(
            "# c\nmsgid \"\"\nmsgstr \"Header: x\\n\"\n\nmsgid \"a \\\"b\\\"\"\nmsgstr \"c\\nd\"\n\nmsgid \"long \"\n\"text\"\nmsgstr \"lang \"\n\"er Text\"\n\nmsgid \"empty\"\nmsgstr \"\"\n",
        );
        assert_eq!(m.get("a \"b\"").map(String::as_str), Some("c\nd"));
        assert_eq!(m.get("long text").map(String::as_str), Some("lang er Text"));
        assert!(!m.contains_key("empty") && !m.contains_key(""), "untranslated entries and the header are skipped");
    }

    #[test]
    fn placeholders_are_filled_and_unknown_ones_stay_visible() {
        assert_eq!(fill("{0} of {1}", &["3".into(), "9".into()]), "3 of 9");
        assert_eq!(fill("{1} before {0}", &["a".into(), "b".into()]), "b before a");
        assert_eq!(fill("keep {2}", &["a".into()]), "keep {2}");
    }

    #[test]
    fn the_system_locale_selects_the_language_with_a_defined_order_and_fallback() {
        assert_eq!(from_locale(&env(&[("LANG", "de_DE.UTF-8")])), "de");
        assert_eq!(from_locale(&env(&[("LANG", "en_US.UTF-8")])), "en");
        assert_eq!(from_locale(&env(&[("LANG", "fr_FR.UTF-8")])), "en", "unsupported language → English");
        assert_eq!(from_locale(&env(&[("LANG", "de_DE.UTF-8"), ("LC_ALL", "en_GB.UTF-8")])), "en", "LC_ALL wins over LANG");
        assert_eq!(from_locale(&env(&[("LANGUAGE", "fr:de:en")])), "de", "first supported entry of the LANGUAGE list");
        assert_eq!(from_locale(&env(&[("LC_ALL", "C"), ("LANG", "de_DE.UTF-8")])), "en", "the C locale means no translation");
        assert_eq!(from_locale(&env(&[])), "en");
        assert_eq!(from_locale(&env(&[("LANG", "de_AT@euro")])), "de");
    }

    #[test]
    fn windows_locale_names_select_the_language_with_english_as_the_fallback() {
        for (tag, lang) in [("de-DE", "de"), ("de-AT", "de"), ("de-CH", "de"), ("en-US", "en"), ("en-GB", "en"), ("fr-FR", ""), ("ja-JP", ""), ("pt-BR", "")] {
            assert_eq!(supported(tag).unwrap_or(FALLBACK), if lang.is_empty() { "en" } else { lang }, "{tag}");
        }
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_its_display_language() {
        let tag = windows_display_language().expect("Windows answers");
        assert!(tag.len() >= 2 && tag.chars().next().unwrap().is_ascii_alphabetic(), "{tag}");
        assert!(LANGUAGES.iter().any(|(c, _, _)| *c == resolve(None, &|_| None)));
    }

    #[test]
    fn an_explicit_setting_beats_the_locale_and_auto_follows_it() {
        let de = env(&[("LANG", "de_DE.UTF-8")]);
        assert_eq!(resolve(None, &de), "de");
        assert_eq!(resolve(Some("auto"), &de), "de");
        assert_eq!(resolve(Some("en"), &de), "en");
        assert_eq!(resolve(Some("de-AT"), &env(&[])), "de");
        assert_eq!(resolve(Some("fr"), &de), "en", "an explicit but unavailable language falls back to English, not to the locale");
        assert_eq!(resolve(Some(""), &de), "de");
    }

    #[test]
    fn a_missing_translation_falls_back_to_the_english_id_and_never_to_another_language() {
        let de = Catalog::new("de");
        assert_eq!(de.tr("No such message id in any catalog {0}"), "No such message id in any catalog {0}");
        assert_eq!(de.render(&Msg::new("No such message id in any catalog {0}", ["x"])), "No such message id in any catalog x");
        assert_eq!(Catalog::new("xx").code(), "en");
        assert!(Catalog::new("en").has("anything"));
        assert!(!de.has("No such message id in any catalog {0}"));
    }

    #[test]
    fn numbers_follow_the_language_and_keep_their_values() {
        let (de, en) = (Catalog::new("de"), Catalog::new("en"));
        assert_eq!(de.bytes(41_200_000), "41,2 MB");
        assert_eq!(en.bytes(41_200_000), "41.2 MB");
        assert_eq!(de.bytes(512), "512 B");
        assert_eq!((de.percent(63), en.percent(63)), ("63 %".to_string(), "63%".to_string()));
        assert_eq!(de.duration(65), "1 min 5 s");
        assert_eq!(en.duration(3), "3 s");
        assert_eq!(en.duration(7440), "2 h 4 min");
    }

    #[test]
    fn every_registered_language_has_a_parsable_catalog() {
        for (code, name, po) in LANGUAGES {
            assert!(!name.is_empty());
            if *code != FALLBACK {
                assert!(parse_po(po).len() > 20, "{code}: catalog is empty or unparsable");
            }
        }
    }

    // ---- completeness: every message id in the sources is translated in every language, and no entry is stale ----

    /// The string literal that starts right after `open` in `src`, for every occurrence of `open`.
    /// `rustfmt` may move a long literal onto its own line (`Msg::new(\n    "text", …)`): pull the literal back behind the opening parenthesis so the scan sees one shape.
    fn unwrap_calls(src: &str) -> String {
        let mut out = String::with_capacity(src.len());
        let chars: Vec<char> = src.chars().collect();
        let mut i = 0;
        while i < chars.len() {
            out.push(chars[i]);
            if chars[i] == '(' {
                let mut j = i + 1;
                while j < chars.len() && chars[j].is_whitespace() {
                    j += 1;
                }
                if j < chars.len() && chars[j] == '"' && j > i + 1 {
                    i = j;
                    continue;
                }
            }
            i += 1;
        }
        out
    }

    fn literals_after(src: &str, open: &str) -> Vec<String> {
        let mut out = Vec::new();
        let unwrapped = unwrap_calls(src);
        let mut rest = unwrapped.as_str();
        while let Some(i) = rest.find(open) {
            rest = &rest[i + open.len()..];
            let mut lit = String::new();
            let mut chars = rest.chars();
            while let Some(c) = chars.next() {
                match c {
                    '"' => break,
                    '\\' => match chars.next() {
                        Some('n') => lit.push('\n'),
                        Some(o) => lit.push(o),
                        None => break,
                    },
                    c => lit.push(c),
                }
            }
            out.push(lit);
        }
        out
    }

    /// All message ids the program can show, collected from its sources and from the library's own tables.
    fn all_message_ids() -> std::collections::BTreeSet<String> {
        let mut ids = std::collections::BTreeSet::new();
        let rust = [include_str!("main.rs"), include_str!("model.rs"), include_str!("messages.rs")];
        for src in rust {
            // the unit-test modules use ids only to exercise the machinery
            let code = src.split("#[cfg(test)]\nmod tests").next().unwrap();
            for open in ["Msg::plain(\"", "Msg::new(\"", "Msg { id: \"", " m(\"", "(m(\""] {
                ids.extend(literals_after(code, open));
            }
        }
        ids.extend(literals_after(include_str!("../ui/launcher.slint"), "@tr(\""));
        // dynamic ids: progress phases (main.rs `phase_id`), library step names, the library's explanations of missing libraries
        ids.extend(["Checking", "Downloading", "Verifying", "Installing"].map(String::from));
        ids.extend(literals_after(include_str!("../../launcher/src/prefix.rs"), "step(Phase::Checking, \""));
        ids.extend(literals_after(include_str!("../../launcher/src/prefix.rs"), "step(Phase::Installing, \""));
        ids.extend(literals_after(include_str!("../../launcher/src/prefix.rs"), "step(Phase::Verifying, \""));
        ids.extend(literals_after(include_str!("../../launcher/src/prefix.rs"), "step: \""));
        let themes = include_str!("theme.rs").split("#[cfg(test)]\nmod tests").next().unwrap();
        ids.extend(literals_after(themes, "name: \""));
        ids.extend(literals_after(themes, "description: \""));
        ids.extend(tsw_launch::doctor::X11_LIBS.iter().map(|(_, _, why)| why.to_string()));
        ids.remove("");
        ids
    }

    #[test]
    fn every_message_id_is_translated_in_every_language_with_the_same_placeholders() {
        let ids = all_message_ids();
        assert!(ids.len() > 100, "the source scan found only {} ids", ids.len());
        let placeholders = |s: &str| {
            let mut v: Vec<String> = (0..10).map(|i| format!("{{{i}}}")).filter(|p| s.contains(p.as_str())).collect();
            v.sort();
            v
        };
        for (code, _, _) in LANGUAGES.iter().filter(|(c, _, _)| *c != FALLBACK) {
            let cat = Catalog::new(code);
            let missing: Vec<&String> = ids.iter().filter(|id| !cat.has(id)).collect();
            assert!(missing.is_empty(), "{code}: untranslated message ids:\n{}", missing.iter().map(|m| format!("  {m}")).collect::<Vec<_>>().join("\n"));
            for id in &ids {
                assert_eq!(placeholders(id), placeholders(cat.tr(id)), "{code}: placeholders differ for {id:?}");
            }
        }
    }

    #[test]
    fn no_catalog_entry_is_left_over_after_a_message_was_removed() {
        let ids = all_message_ids();
        for (code, _, po) in LANGUAGES.iter().filter(|(c, _, _)| *c != FALLBACK) {
            let stale: Vec<String> = parse_po(po).into_keys().filter(|k| !ids.contains(k)).collect();
            assert!(stale.is_empty(), "{code}: entries no source uses: {stale:?}");
        }
    }
}
