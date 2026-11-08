//! Per-user client preferences that the launcher may set from the outside.
//!
//! The original client keeps user preferences in `<AppData>/Local/Funcom/TSW/Prefs/Prefs_3.xml` (the same
//! `<Root><Value name=".." value=".."/></Root>` shape as `Data/Gui/Default/MainPrefs.xml`) and derives a binary
//! `Prefs_3.bxml` cache from it. Nothing here touches the client executable, the RDB or any game file: only that
//! per-user file is written, and only the one entry that is asked for.
//!
//! Used for the outdated-graphics-driver warning (documentation/architecture.md): the client
//! shows the dialog only while the preference `DriverUpdate_VersionToNotShowAgain` differs from the name of the
//! OS-dependent "minimum driver" variable it checked (`MinimumDriver_Seven_ATI_DX10` for Windows 7 and newer, which is
//! what Wine reports). Ticking "do not notify again" stores exactly that name; we store it up front.

use std::io;
use std::path::{Path, PathBuf};

pub const FILE: &str = "Prefs_3.xml";
const CACHE: &str = "Prefs_3.bxml";
pub const DRIVER_PREF: &str = "DriverUpdate_VersionToNotShowAgain";
pub const DRIVER_VALUE: &str = "MinimumDriver_Seven_ATI_DX10";
pub const RESOLUTION_PREF: &str = "DisplayResolution";
pub const DISPLAY_MODE_PREF: &str = "DisplayMode";

/// The client's `DisplayMode` preference as measured on 2026-10-30 (documentation/architecture.md section 2):
/// 0 = X11 `FULLSCREEN`, 1 = decorated window, 2 = also monitor-filling (probably borderless; Wine cannot tell it from 0).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DisplayMode {
    Fullscreen,
    Windowed,
    Borderless,
}

impl DisplayMode {
    pub fn parse(v: &str) -> Result<DisplayMode, String> {
        match v {
            "fullscreen" => Ok(DisplayMode::Fullscreen),
            "windowed" => Ok(DisplayMode::Windowed),
            "borderless" => Ok(DisplayMode::Borderless),
            _ => Err("display_mode must be fullscreen, windowed or borderless".into()),
        }
    }
    pub fn text(self) -> &'static str {
        match self {
            DisplayMode::Fullscreen => "fullscreen",
            DisplayMode::Windowed => "windowed",
            DisplayMode::Borderless => "borderless",
        }
    }
    pub fn pref_value(self) -> &'static str {
        match self {
            DisplayMode::Fullscreen => "0",
            DisplayMode::Windowed => "1",
            DisplayMode::Borderless => "2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Unchanged,
    Created,
    Updated,
}

fn safe_token(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '.' | '-' | '(' | ')' | ','))
}

/// Attribute text for a plain value such as `IPoint(1920,1200)`: `"IPoint(1920,1200)"`.
pub fn attr_plain(value: &str) -> Result<String, String> {
    safe_token(value).then(|| format!("\"{value}\"")).ok_or_else(|| "preference value may only use letters, digits, '_', '.', '-', '(', ')', ','".to_string())
}

/// Attribute text for a string value, in the form the client itself writes: `'&quot;Name&quot;'`.
pub fn attr_string(value: &str) -> Result<String, String> {
    safe_token(value).then(|| format!("'&quot;{value}&quot;'")).ok_or_else(|| "preference value may only use letters, digits, '_', '.', '-', '(', ')', ','".to_string())
}

/// Return `xml` with `<Value name="name" value=ATTR />` (the client's own canonical form, so repeated launches change
/// nothing once the client has saved the file) present exactly once. `attr` comes from [`attr_plain`]/[`attr_string`].
/// Other entries are kept as they are.
pub fn set_value(xml: &str, name: &str, attr: &str) -> Result<String, String> {
    if !safe_token(name) {
        return Err("preference name may only use letters, digits, '_', '.', '-'".into());
    }
    let entry = format!("<Value name=\"{name}\" value={attr} />");
    let marker = format!("name=\"{name}\"");
    if let Some(at) = xml.find(&marker) {
        let start = xml[..at].rfind("<Value").ok_or("malformed preferences file: name without <Value")?;
        let end = xml[at..].find("/>").map(|e| at + e + 2).ok_or("malformed preferences file: unterminated <Value")?;
        let mut out = String::with_capacity(xml.len() + entry.len());
        out.push_str(&xml[..start]);
        out.push_str(&entry);
        out.push_str(&xml[end..]);
        return Ok(out);
    }
    match xml.rfind("</Root>") {
        Some(at) => Ok(format!("{}  {entry}\n{}", &xml[..at], &xml[at..])),
        None if xml.trim().is_empty() => Ok(format!("<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\" ?>\n<Root>\n  {entry}\n</Root>\n")),
        None => Err("malformed preferences file: no </Root>".into()),
    }
}

/// The value text of preference `name` in `xml` (`value="0"` -> `0`; quotes of any kind stripped), `None` if absent.
pub fn get_value(xml: &str, name: &str) -> Option<String> {
    let at = xml.find(&format!("name=\"{name}\""))?;
    let end = xml[at..].find("/>").map(|e| at + e)?;
    let entry = &xml[at..end];
    let v = entry.split_once("value=")?.1.trim();
    Some(v.trim_matches(|c| c == '"' || c == '\'').replace("&quot;", "").trim().to_string())
}

/// The client's saved `DisplayMode` preference in `dir` (the Prefs folder): 0 fullscreen, 1 windowed, 2 borderless.
pub fn read_display_mode(dir: &Path) -> Option<DisplayMode> {
    let xml = std::fs::read_to_string(dir.join(FILE)).ok()?;
    match get_value(&xml, DISPLAY_MODE_PREF)?.as_str() {
        "0" => Some(DisplayMode::Fullscreen),
        "1" => Some(DisplayMode::Windowed),
        "2" => Some(DisplayMode::Borderless),
        _ => None,
    }
}

/// Value text of the `IPoint` preference form used by the client: `IPoint(1920,1200)`.
pub fn ipoint(w: u32, h: u32) -> String {
    format!("IPoint({w},{h})")
}

/// Directory of the user preferences inside a Wine prefix. Proton/GE-Proton prefixes use the user `steamuser`; plain
/// Wine prefixes use the host user name, so the directory that already has an `AppData` is preferred.
pub fn wine_prefs_dir(prefix: &Path) -> PathBuf {
    let users = prefix.join("drive_c").join("users");
    let mut name = "steamuser".to_string();
    if !users.join(&name).join("AppData").is_dir() {
        if let Ok(rd) = std::fs::read_dir(&users) {
            let mut found: Vec<String> = rd.flatten().filter(|e| e.path().join("AppData").is_dir()).filter_map(|e| e.file_name().into_string().ok()).filter(|n| n != "Public").collect();
            found.sort();
            if let Some(n) = found.into_iter().next() {
                name = n;
            }
        }
    }
    users.join(name).join("AppData").join("Local").join("Funcom").join("TSW").join("Prefs")
}

/// Native Windows: `%LOCALAPPDATA%\Funcom\TSW\Prefs`.
pub fn native_prefs_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").filter(|v| !v.is_empty()).map(|d| PathBuf::from(d).join("Funcom").join("TSW").join("Prefs"))
}

/// What `ensure_value` would do, without writing anything.
pub fn plan(dir: &Path, name: &str, attr: &str) -> Result<Outcome, String> {
    let path = dir.join(FILE);
    match std::fs::read_to_string(&path) {
        Ok(old) => Ok(if set_value(&old, name, attr)? == old { Outcome::Unchanged } else { Outcome::Updated }),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Outcome::Created),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Make sure `Prefs_3.xml` in `dir` holds `name = value`. The file is replaced atomically; the derived `.bxml`
/// cache is removed only when the XML changed (the client rebuilds it).
pub fn ensure_value(dir: &Path, name: &str, attr: &str) -> Result<Outcome, String> {
    let path = dir.join(FILE);
    let outcome = plan(dir, name, attr)?;
    if outcome == Outcome::Unchanged {
        return Ok(outcome);
    }
    let old = std::fs::read_to_string(&path).unwrap_or_default();
    let new = set_value(&old, name, attr)?;
    std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
    let tmp = dir.join(format!("{FILE}.tsw-launch.tmp"));
    std::fs::write(&tmp, new).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, &path).map_err(|e| format!("cannot replace {}: {e}", path.display()))?;
    let _ = std::fs::remove_file(dir.join(CACHE));
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-launch-prefs-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn driver_attr() -> String {
        attr_string(DRIVER_VALUE).unwrap()
    }

    /// The client's own rewrite of the two entries (observed after a real run).
    const CLIENT_SAVED: &str = "<?xml version=\"1.0\" encoding=\"UTF-8\" standalone=\"yes\" ?>\n<Root>\n    <Value name=\"DisplayResolution\" value=\"IPoint(1920,1200)\" />\n    <Value name=\"DriverUpdate_VersionToNotShowAgain\" value='&quot;MinimumDriver_Seven_ATI_DX10&quot;' />\n    <Value name=\"Language\" value='&quot;de&quot;' />\n</Root>\n";

    #[test]
    fn creates_a_complete_file_from_nothing_in_the_clients_canonical_form() {
        let x = set_value("", DRIVER_PREF, &driver_attr()).unwrap();
        assert!(x.contains("<Root>") && x.contains("</Root>"));
        assert!(x.contains("<Value name=\"DriverUpdate_VersionToNotShowAgain\" value='&quot;MinimumDriver_Seven_ATI_DX10&quot;' />"));
    }

    #[test]
    fn the_clients_own_saved_file_is_already_correct_and_stays_byte_identical() {
        assert_eq!(set_value(CLIENT_SAVED, DRIVER_PREF, &driver_attr()).unwrap(), CLIENT_SAVED);
        assert_eq!(set_value(CLIENT_SAVED, RESOLUTION_PREF, &attr_plain(&ipoint(1920, 1200)).unwrap()).unwrap(), CLIENT_SAVED);
        let changed = set_value(CLIENT_SAVED, RESOLUTION_PREF, &attr_plain(&ipoint(1280, 720)).unwrap()).unwrap();
        assert!(changed.contains("IPoint(1280,720)") && changed.contains("name=\"Language\""), "only the one entry changes");
    }

    #[test]
    fn replaces_an_existing_entry_and_keeps_others() {
        let old = "<Root>\n  <Value name=\"A\" value=\"1\"/>\n  <Value name=\"DriverUpdate_VersionToNotShowAgain\" value=\"none\"/>\n  <Value name=\"B\" value=\"2\"/>\n</Root>\n";
        let n = set_value(old, DRIVER_PREF, &driver_attr()).unwrap();
        assert!(n.contains("name=\"A\" value=\"1\"") && n.contains("name=\"B\" value=\"2\"") && !n.contains("none"));
        assert_eq!(n.matches(DRIVER_PREF).count(), 1);
        assert_eq!(set_value(&n, DRIVER_PREF, &driver_attr()).unwrap(), n, "idempotent");
    }

    #[test]
    fn appends_to_a_foreign_file_before_the_closing_tag() {
        let n = set_value("<Root>\n  <Value name=\"A\" value=\"1\"/>\n</Root>\n", "X", &attr_plain("y").unwrap()).unwrap();
        assert!(n.find("name=\"X\"").unwrap() < n.find("</Root>").unwrap());
        assert!(n.contains("name=\"A\""));
    }

    #[test]
    fn refuses_unsafe_input_and_broken_files() {
        assert!(set_value("", "a\"b", "\"v\"").is_err());
        assert!(attr_plain("v\"/><x").is_err() && attr_string("a b").is_err());
        assert!(set_value("garbage", "n", "\"v\"").is_err());
    }

    #[test]
    fn ensure_writes_atomically_reports_outcomes_and_drops_only_the_cache() {
        let d = tmp("ensure");
        let dir = d.join("p");
        let a = driver_attr();
        assert_eq!(plan(&dir, DRIVER_PREF, &a).unwrap(), Outcome::Created);
        assert_eq!(ensure_value(&dir, DRIVER_PREF, &a).unwrap(), Outcome::Created);
        std::fs::write(dir.join(CACHE), b"cache").unwrap();
        assert_eq!(ensure_value(&dir, DRIVER_PREF, &a).unwrap(), Outcome::Unchanged);
        assert!(dir.join(CACHE).exists(), "unchanged XML keeps the cache");
        std::fs::write(dir.join(FILE), CLIENT_SAVED).unwrap();
        assert_eq!(ensure_value(&dir, DRIVER_PREF, &a).unwrap(), Outcome::Unchanged, "no churn after the client saved its own format");
        std::fs::write(dir.join(FILE), "<Root>\n  <Value name=\"DriverUpdate_VersionToNotShowAgain\" value=\"none\"/>\n</Root>\n").unwrap();
        assert_eq!(ensure_value(&dir, DRIVER_PREF, &a).unwrap(), Outcome::Updated);
        assert!(!dir.join(CACHE).exists());
        assert!(!dir.join(format!("{FILE}.tsw-launch.tmp")).exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn resolution_value_is_written_in_the_clients_ipoint_form() {
        let d = tmp("res");
        let dir = d.join("p");
        let a = attr_plain(&ipoint(1920, 1200)).unwrap();
        assert_eq!(ipoint(1920, 1200), "IPoint(1920,1200)");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "<Root>\n  <Value name=\"DisplayResolution\" value=\"IPoint(1024,768)\" />\n</Root>\n").unwrap();
        assert_eq!(ensure_value(&dir, RESOLUTION_PREF, &a).unwrap(), Outcome::Updated);
        assert!(std::fs::read_to_string(dir.join(FILE)).unwrap().contains("IPoint(1920,1200)"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_display_mode_maps_to_the_measured_pref_values_and_changes_only_that_entry() {
        for (t, v) in [("fullscreen", "0"), ("windowed", "1"), ("borderless", "2")] {
            assert_eq!(DisplayMode::parse(t).unwrap().pref_value(), v);
            assert_eq!(DisplayMode::parse(t).unwrap().text(), t);
        }
        assert!(DisplayMode::parse("2").is_err() && DisplayMode::parse("").is_err());
        let changed = set_value(CLIENT_SAVED, DISPLAY_MODE_PREF, &attr_plain("2").unwrap()).unwrap();
        assert!(changed.contains("<Value name=\"DisplayMode\" value=\"2\" />") && changed.contains("IPoint(1920,1200)") && changed.contains("name=\"Language\""));
        assert_eq!(set_value(&changed, DISPLAY_MODE_PREF, &attr_plain("2").unwrap()).unwrap(), changed, "idempotent");
    }

    #[test]
    fn finds_the_wine_user_directory() {
        let d = tmp("users");
        let u = d.join("drive_c/users");
        std::fs::create_dir_all(u.join("alice/AppData")).unwrap();
        std::fs::create_dir_all(u.join("Public")).unwrap();
        assert!(wine_prefs_dir(&d).starts_with(u.join("alice")));
        std::fs::create_dir_all(u.join("steamuser/AppData")).unwrap();
        assert!(wine_prefs_dir(&d).starts_with(u.join("steamuser")));
        let e = tmp("empty");
        assert!(wine_prefs_dir(&e).starts_with(e.join("drive_c/users/steamuser")), "fresh prefix: Proton default");
        let _ = std::fs::remove_dir_all(&d);
        let _ = std::fs::remove_dir_all(&e);
    }

    #[test]
    fn saved_values_can_be_read_back_and_the_display_mode_is_decoded() {
        let xml = "<Root>\n  <Value name=\"DisplayMode\" value=\"2\" />\n  <Value name=\"Other\" value='&quot;x y&quot;' />\n</Root>\n";
        assert_eq!(get_value(xml, "DisplayMode").as_deref(), Some("2"));
        assert_eq!(get_value(xml, "Other").as_deref(), Some("x y"));
        assert_eq!(get_value(xml, "Missing"), None);
        let d = tmp("readmode");
        assert_eq!(read_display_mode(&d), None, "no file");
        for (v, m) in [("0", DisplayMode::Fullscreen), ("1", DisplayMode::Windowed), ("2", DisplayMode::Borderless)] {
            std::fs::write(d.join(FILE), format!("<Root>\n  <Value name=\"DisplayMode\" value=\"{v}\" />\n</Root>\n")).unwrap();
            assert_eq!(read_display_mode(&d), Some(m));
        }
        std::fs::write(d.join(FILE), "<Root>\n  <Value name=\"DisplayMode\" value=\"7\" />\n</Root>\n").unwrap();
        assert_eq!(read_display_mode(&d), None, "an unknown value is not guessed");
        let _ = std::fs::remove_dir_all(&d);
    }
}
