//! Optional configuration file: plain `key = value` lines (no external parser, no secrets).
//!
//! ```text
//! # tsw-launch configuration
//! client_exe = /path/to/TheSecretWorldDX11.exe
//! runner     = wine            # native | wine | /path/to/wine
//! prefix     = /path/to/prefix # WINEPREFIX (wine runners)
//! dx         = 11              # 9 | 11
//! no_console = true
//! resolution = 1920x1200           # optional: sets the client's DisplayResolution (default: the virtual desktop size)
//! virtual_desktop = 1920x1200      # optional: run inside Wine's virtual desktop (compatibility mode, see documentation)
//! skip_intro = true                # pass the client's own -novideo switch (skips the start-up videos)
//! language = auto                  # GUI language: auto (system locale) or a language code such as de, en
//! fps_limit = 60                    # frame-rate limit of the game: 30..240 or unlimited (default 60)
//! fps_hud = false                   # show DXVK's FPS counter in the game
//! theme = templars                  # GUI colour theme: templars (default when absent) | illuminati | dragon | neutral; cosmetic only
//! display_mode = borderless         # optional, off by default: set the client's DisplayMode at every start (fullscreen | windowed | borderless)
//! wm_decorations = auto            # window-manager decorations of the game window: auto (default; removed for a full-screen game = Alt+Tab fix) | on | off
//! gamescope = false                 # optional plain gamescope window, no scaling (default off, see gamescope.rs)
//! gamescope_output = 1920x1200      # its resolution, inside and outside (default: the monitor's native size)
//! # fsr, fsr_internal, fsr_output, fsr_sharpness: FSR was removed; these old keys are still read and ignored
//! suppress_driver_warning = true   # default; pre-sets the client's own "do not notify again" preference
//! env.WINEDLLOVERRIDES = d3d11,dxgi=n,b   # extra environment for the client process (NAME after "env.")
//! ```
//!
//! Credentials never belong here: keys that look like `loginkey`, `statindex`, `token`, `password`, ... are refused.
//! Command-line options override the file.

use std::path::{Path, PathBuf};

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Config {
    pub client_exe: Option<PathBuf>,
    pub runner: Option<String>,
    pub prefix: Option<PathBuf>,
    pub dx11: Option<bool>,
    pub no_console: Option<bool>,
    pub suppress_driver_warning: Option<bool>,
    pub skip_intro: Option<bool>,
    /// GUI language: `auto` or a language code (`de`, `en`, `pt-BR`, ...). The CLI ignores it; the GUI stores its choice here.
    pub language: Option<String>,
    /// GUI colour theme id (cosmetic only; never changes the game or the player's faction). Absent = the original Templar red.
    pub theme: Option<String>,
    /// Frame-rate limit for the game process (see `fps.rs`); absent = the default of 60 FPS, which users can change.
    pub fps_limit: Option<crate::fps::FpsLimit>,
    /// Show DXVK's FPS counter in the game (to verify the limit); absent = off.
    pub fps_hud: Option<bool>,
    /// Diagnostic logging (off by default): `off`, `basic` or `detailed` (see `diaglog.rs`).
    pub diagnostics: Option<crate::diaglog::DiagLevel>,
    /// Folder with the 32-bit DLLs of the compiler profile (`prefix native-install`); a path only, nothing is copied from it unless asked.
    pub native_dir: Option<PathBuf>,
    pub virtual_desktop: Option<(u32, u32)>,
    pub resolution: Option<(u32, u32)>,
    /// GUI-managed per-game Wine desktop; absent leaves the game resolution alone.
    pub game_resolution: Option<(u32, u32)>,
    /// Explicit game-managed choice; legacy display keys remain stored but do not override this policy.
    pub game_managed: bool,
    /// Opt-in: set the client's `DisplayMode` preference at every start (absent = the client's own, saved choice is left alone).
    pub display_mode: Option<crate::prefs::DisplayMode>,
    /// Client patch source override (https URL ending in `/`); absent = the project's GitHub release (see `project.rs`).
    pub patch_source: Option<String>,
    /// Apply client patches automatically before the game starts (default: on, but only if a trusted key exists in the launcher).
    pub patch_auto: Option<bool>,
    /// Window-manager decorations of the game window (see `windowing.rs`); absent = `auto`.
    pub wm_decorations: Option<crate::windowing::WmDecorations>,
    /// Optional plain Gamescope window, internal = output resolution, no scaling (see `gamescope.rs`); absent = off (the normal start).
    pub gamescope: Option<bool>,
    /// Resolution of that Gamescope window; absent = the monitor's native size.
    pub gamescope_output: Option<(u32, u32)>,
    pub env: Vec<(String, String)>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ConfigError {
    Syntax(usize, String),
    ForbiddenKey(usize, String),
    Io(String),
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Syntax(n, m) => write!(f, "config line {n}: {m}"),
            ConfigError::ForbiddenKey(n, k) => {
                write!(f, "config line {n}: {k:?} looks like a credential; pass such values with --arg-env at start time, never in a file")
            }
            ConfigError::Io(m) => write!(f, "{m}"),
        }
    }
}

const FORBIDDEN: &[&str] = &["loginkey", "statindex", "token", "password", "passwd", "secret", "cookie", "session", "ticket", "credential"];

fn forbidden(key: &str) -> bool {
    let k = key.to_ascii_lowercase();
    FORBIDDEN.iter().any(|f| k.contains(f))
}

fn parse_bool(v: &str) -> Option<bool> {
    match v.to_ascii_lowercase().as_str() {
        "true" | "yes" | "1" | "on" => Some(true),
        "false" | "no" | "0" | "off" => Some(false),
        _ => None,
    }
}

/// `1920x1200` -> (1920, 1200); both sides 320..=16384.
pub fn parse_size(v: &str) -> Option<(u32, u32)> {
    let (w, h) = v.split_once('x')?;
    let (w, h): (u32, u32) = (w.trim().parse().ok()?, h.trim().parse().ok()?);
    ((320..=16384).contains(&w) && (320..=16384).contains(&h)).then_some((w, h))
}

pub fn parse(text: &str) -> Result<Config, ConfigError> {
    let mut c = Config::default();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(ConfigError::Syntax(n, "expected key = value".into()));
        };
        let (k, v) = (k.trim(), v.trim());
        if forbidden(k) || (k.starts_with("env.") && forbidden(&k[4..])) {
            return Err(ConfigError::ForbiddenKey(n, k.to_string()));
        }
        match k {
            "client_exe" => c.client_exe = Some(PathBuf::from(v)),
            "runner" => c.runner = Some(v.to_string()),
            "prefix" => c.prefix = Some(PathBuf::from(v)),
            "dx" => {
                c.dx11 = Some(match v {
                    "11" => true,
                    "9" => false,
                    _ => return Err(ConfigError::Syntax(n, "dx must be 9 or 11".into())),
                })
            }
            "no_console" => c.no_console = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "no_console must be true or false".into()))?),
            "game_resolution" => {
                c.game_managed = v == "game";
                c.game_resolution = if c.game_managed { None } else { Some(parse_size(v).ok_or_else(|| ConfigError::Syntax(n, "game_resolution must be game or a size like 1920x1080".into()))?) };
            }
            "resolution" => c.resolution = Some(parse_size(v).ok_or_else(|| ConfigError::Syntax(n, "resolution must look like 1920x1200".into()))?),
            "virtual_desktop" => c.virtual_desktop = Some(parse_size(v).ok_or_else(|| ConfigError::Syntax(n, "virtual_desktop must look like 1920x1200".into()))?),
            "display_mode" => c.display_mode = Some(crate::prefs::DisplayMode::parse(v).map_err(|m| ConfigError::Syntax(n, m))?),
            "patch_source" => {
                if !(v.starts_with("https://") && v.ends_with('/') && v.len() < 300 && v.chars().all(|ch| ch.is_ascii_graphic())) {
                    return Err(ConfigError::Syntax(n, "patch_source must be an https URL ending in /".into()));
                }
                c.patch_source = Some(v.to_string());
            }
            "patch_auto" => c.patch_auto = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "patch_auto must be true or false".into()))?),
            "gamescope" => c.gamescope = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "gamescope must be true or false".into()))?),
            "gamescope_output" => c.gamescope_output = Some(parse_size(v).ok_or_else(|| ConfigError::Syntax(n, "gamescope_output must look like 1920x1200".into()))?),
            // FSR was removed (2026-10-31): the old keys stay readable, whatever they hold, and have no effect
            "fsr" | "fsr_internal" | "fsr_output" | "fsr_sharpness" => {}
            "skip_intro" => c.skip_intro = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "skip_intro must be true or false".into()))?),
            "language" => {
                let ok = v == "auto" || (!v.is_empty() && v.len() <= 16 && v.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'));
                if !ok {
                    return Err(ConfigError::Syntax(n, "language must be auto or a code like de, en, pt-BR".into()));
                }
                c.language = Some(v.to_string());
            }
            "fps_limit" => c.fps_limit = Some(crate::fps::parse(v).map_err(|m| ConfigError::Syntax(n, m))?),
            "diagnostics" => c.diagnostics = Some(crate::diaglog::DiagLevel::parse(v).map_err(|m| ConfigError::Syntax(n, m))?),
            "native_dir" => c.native_dir = Some(PathBuf::from(v)),
            "wm_decorations" => c.wm_decorations = Some(crate::windowing::WmDecorations::parse(v).map_err(|m| ConfigError::Syntax(n, m))?),
            "fps_hud" => c.fps_hud = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "fps_hud must be true or false".into()))?),
            "theme" => {
                let ok = !v.is_empty() && v.len() <= 24 && v.chars().all(|ch| ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_');
                if !ok {
                    return Err(ConfigError::Syntax(n, "theme must be a lower-case id like templars, illuminati, dragon, neutral".into()));
                }
                c.theme = Some(v.to_string());
            }
            "suppress_driver_warning" => c.suppress_driver_warning = Some(parse_bool(v).ok_or_else(|| ConfigError::Syntax(n, "suppress_driver_warning must be true or false".into()))?),
            _ if k.starts_with("env.") && k.len() > 4 => {
                let name = &k[4..];
                if !name.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                    return Err(ConfigError::Syntax(n, "environment variable names may use letters, digits and _".into()));
                }
                c.env.push((name.to_string(), v.to_string()));
            }
            other => return Err(ConfigError::Syntax(n, format!("unknown key {other:?}"))),
        }
    }
    Ok(c)
}

/// Diagnostic representation: saved environment VALUES are never serialised.
pub fn diagnostic_text(c: &Config) -> String {
    let mut reduced = c.clone();
    reduced.env.clear();
    let mut text = to_text(&reduced);
    for (name, _) in &c.env {
        text.push_str(&format!("# environment name: {name} (value omitted)\n"));
    }
    text
}

/// Render the persisted configuration, including environment entries.
pub fn to_text(c: &Config) -> String {
    let mut s = String::from("# tsw-launch configuration (written by the launcher; credentials never belong here)\n");
    let mut put = |k: &str, v: String| s.push_str(&format!("{k} = {v}\n"));
    if let Some(v) = &c.client_exe {
        put("client_exe", v.display().to_string());
    }
    if let Some(v) = &c.runner {
        put("runner", v.clone());
    }
    if let Some(v) = &c.prefix {
        put("prefix", v.display().to_string());
    }
    if let Some(v) = c.dx11 {
        put("dx", if v { "11" } else { "9" }.into());
    }
    if let Some(v) = c.no_console {
        put("no_console", v.to_string());
    }
    if let Some((w, h)) = c.virtual_desktop {
        put("virtual_desktop", format!("{w}x{h}"));
    }
    if c.game_managed {
        put("game_resolution", "game".into());
    } else if let Some((w, h)) = c.game_resolution {
        put("game_resolution", format!("{w}x{h}"));
    }
    if let Some((w, h)) = c.resolution {
        put("resolution", format!("{w}x{h}"));
    }
    if let Some(v) = c.display_mode {
        put("display_mode", v.text().to_string());
    }
    if let Some(v) = &c.patch_source {
        put("patch_source", v.clone());
    }
    if let Some(v) = c.patch_auto {
        put("patch_auto", v.to_string());
    }
    if let Some(v) = c.gamescope {
        put("gamescope", v.to_string());
    }
    if let Some((w, h)) = c.gamescope_output {
        put("gamescope_output", format!("{w}x{h}"));
    }
    if let Some(v) = c.skip_intro {
        put("skip_intro", v.to_string());
    }
    if let Some(v) = &c.language {
        put("language", v.clone());
    }
    if let Some(v) = &c.theme {
        put("theme", v.clone());
    }
    if let Some(v) = c.fps_limit {
        put("fps_limit", v.text());
    }
    if let Some(v) = c.fps_hud {
        put("fps_hud", v.to_string());
    }
    if let Some(v) = c.diagnostics {
        put("diagnostics", v.text().to_string());
    }
    if let Some(v) = &c.native_dir {
        put("native_dir", v.display().to_string());
    }
    if let Some(v) = c.wm_decorations {
        put("wm_decorations", v.text().to_string());
    }
    if let Some(v) = c.suppress_driver_warning {
        put("suppress_driver_warning", v.to_string());
    }
    for (k, v) in &c.env {
        put(&format!("env.{k}"), v.clone());
    }
    s
}

/// Save atomically (temporary file + rename); creates the parent directory.
pub fn save(path: &Path, c: &Config) -> Result<(), ConfigError> {
    let io = |what: &str, e: std::io::Error| ConfigError::Io(format!("{what} {}: {e}", path.display()));
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| io("cannot create directory for", e))?;
    }
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, to_text(c)).map_err(|e| io("cannot write", e))?;
    std::fs::rename(&tmp, path).map_err(|e| io("cannot replace", e))
}

pub fn load(path: &Path) -> Result<Config, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(|e| ConfigError::Io(format!("cannot read config {}: {e}", path.display())))?;
    parse(&text)
}

/// Where the configuration is written: `$TSW_LAUNCH_CONFIG`, else `$XDG_CONFIG_HOME/tsw-launch/config`,
/// `%APPDATA%\tsw-launch\config`, `~/.config/tsw-launch/config`. Unlike [`default_path`] the file need not exist.
pub fn write_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    env("TSW_LAUNCH_CONFIG")
        .or_else(|| env("XDG_CONFIG_HOME").map(|d| d.join("tsw-launch").join("config")))
        .or_else(|| env("APPDATA").map(|d| d.join("tsw-launch").join("config")))
        .or_else(|| env("HOME").map(|d| d.join(".config").join("tsw-launch").join("config")))
}

/// Default location: `$TSW_LAUNCH_CONFIG`, else `$XDG_CONFIG_HOME/tsw-launch/config`, `~/.config/tsw-launch/config`
/// (Linux/macOS) or `%APPDATA%\tsw-launch\config` (Windows). Returns the path only if the file exists.
pub fn default_path() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    let candidate = env("TSW_LAUNCH_CONFIG")
        .or_else(|| env("XDG_CONFIG_HOME").map(|d| d.join("tsw-launch").join("config")))
        .or_else(|| env("APPDATA").map(|d| d.join("tsw-launch").join("config")))
        .or_else(|| env("HOME").map(|d| d.join(".config").join("tsw-launch").join("config")))?;
    candidate.is_file().then_some(candidate)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_all_keys_comments_and_blank_lines() {
        let c = parse("# c\n\nclient_exe = /g/tsw.exe  # trailing\nrunner=wine\nprefix = /p\ndx = 9\nno_console = yes\nsuppress_driver_warning = off\nenv.WINEDLLOVERRIDES = d3d11=n,b\n").unwrap();
        assert_eq!(c.client_exe, Some(PathBuf::from("/g/tsw.exe")));
        assert_eq!((c.runner.as_deref(), c.dx11, c.no_console), (Some("wine"), Some(false), Some(true)));
        assert_eq!(c.suppress_driver_warning, Some(false));
        assert_eq!(parse("virtual_desktop = 1920x1200").unwrap().virtual_desktop, Some((1920, 1200)));
        assert!(parse("virtual_desktop = 10x10").is_err() && parse("virtual_desktop = big").is_err());
        assert_eq!(c.env, vec![("WINEDLLOVERRIDES".to_string(), "d3d11=n,b".to_string())]);
    }

    #[test]
    fn written_configuration_parses_back_identically() {
        let c = parse("client_exe = /g/t.exe\nrunner = /w/wine\nprefix = /p\ndx = 11\nno_console = true\nvirtual_desktop = 1920x1200\nresolution = 1280x720\nskip_intro = true\nsuppress_driver_warning = false\nenv.A_B = x y\n").unwrap();
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        let dir = std::env::temp_dir().join(format!("tsw-cfg-save-{}", std::process::id()));
        let p = dir.join("sub").join("config");
        save(&p, &c).unwrap();
        assert_eq!(load(&p).unwrap(), c);
        assert!(!p.with_extension("tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
        assert!(parse("skip_intro = maybe").is_err());
    }

    #[test]
    fn credentials_are_refused_in_any_spelling() {
        for k in ["loginkey = x", "StatIndex = 1", "env.GAME_TOKEN = x", "password = p", "env.SESSIONID = 1"] {
            assert!(matches!(parse(k), Err(ConfigError::ForbiddenKey(1, _))), "{k}");
        }
    }

    #[test]
    fn the_gui_language_is_stored_validated_and_round_trips() {
        let c = parse("language = de\n").unwrap();
        assert_eq!(c.language.as_deref(), Some("de"));
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        assert_eq!(parse("language = auto\n").unwrap().language.as_deref(), Some("auto"));
        assert_eq!(parse("language = pt-BR\n").unwrap().language.as_deref(), Some("pt-BR"));
        for bad in ["language =\n", "language = ../etc\n", "language = deutsch deutsch\n", "language = aaaaaaaaaaaaaaaaaaaa\n"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
        assert!(!to_text(&Config::default()).contains("language"), "unset stays unset");
    }

    #[test]
    fn the_gui_theme_is_optional_validated_and_old_files_stay_valid() {
        assert_eq!(parse("client_exe = /x\n").unwrap().theme, None, "files without the key keep working (default theme)");
        let c = parse("theme = illuminati\n").unwrap();
        assert_eq!(c.theme.as_deref(), Some("illuminati"));
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        for bad in ["theme =\n", "theme = Red\n", "theme = ../x\n", "theme = aaaaaaaaaaaaaaaaaaaaaaaaaaaa\n"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn errors_name_the_line() {
        assert_eq!(parse("a = 1").unwrap_err(), ConfigError::Syntax(1, "unknown key \"a\"".into()));
        assert!(matches!(parse("dx = 12"), Err(ConfigError::Syntax(1, _))));
        assert!(matches!(parse("client_exe /x"), Err(ConfigError::Syntax(1, _))));
        assert!(matches!(parse("env.BAD-NAME = 1"), Err(ConfigError::Syntax(1, _))));
        assert!(matches!(parse("no_console = maybe"), Err(ConfigError::Syntax(1, _))));
    }

    #[test]
    fn the_fps_limit_is_optional_validated_and_round_trips() {
        use crate::fps::FpsLimit;
        assert_eq!(parse("client_exe = /x\n").unwrap().fps_limit, None, "old files keep working (default 60 is applied at start)");
        assert_eq!(parse("fps_limit = 144\n").unwrap().fps_limit, Some(FpsLimit::Cap(144)));
        assert_eq!(parse("fps_limit = unlimited\n").unwrap().fps_limit, Some(FpsLimit::Unlimited));
        assert_eq!(parse("fps_hud = true\n").unwrap().fps_hud, Some(true));
        for bad in ["fps_limit = 10\n", "fps_limit = 500\n", "fps_limit = fast\n", "fps_hud = maybe\n"] {
            assert!(parse(bad).is_err(), "{bad}");
        }
        let c = parse("fps_limit = 90\nfps_hud = false\nenv.FOO = bar\n").unwrap();
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        assert!(!to_text(&Config::default()).contains("fps"), "unset stays unset");
        use crate::diaglog::DiagLevel;
        assert_eq!(parse("client_exe = /x\n").unwrap().diagnostics, None, "diagnostics are off unless asked for");
        let c = parse("diagnostics = detailed\nnative_dir = /some dir/(x86)\n").unwrap();
        assert_eq!((c.diagnostics, c.native_dir.as_deref().map(|p| p.to_str().unwrap())), (Some(DiagLevel::Detailed), Some("/some dir/(x86)")));
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        assert!(parse("diagnostics = loud\n").is_err());
    }

    #[test]
    fn display_mode_and_patch_source_round_trip_and_unset_stays_unset() {
        let none = parse("client_exe = /x\n").unwrap();
        assert!(!to_text(&none).contains("gamescope") && !to_text(&none).contains("display_mode"), "unset stays unset");
        let dm = parse("display_mode = borderless\n").unwrap();
        assert_eq!(parse(&to_text(&dm)).unwrap(), dm);
        assert!(parse("display_mode = 2\n").is_err());
        let ps = parse("patch_source = https://example.org/p/\npatch_auto = off\n").unwrap();
        assert_eq!(parse(&to_text(&ps)).unwrap(), ps);
        for bad in ["patch_source = http://example.org/p/\n", "patch_source = https://example.org/p\n", "patch_source = file:///x/\n", "patch_auto = maybe\n"] {
            assert!(parse(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn the_window_decoration_option_round_trips_and_rejects_garbage() {
        use crate::windowing::WmDecorations;
        assert_eq!(parse("client_exe = /x\n").unwrap().wm_decorations, None, "absent = auto");
        for (text, want) in [("auto", WmDecorations::Auto), ("on", WmDecorations::On), ("off", WmDecorations::Off)] {
            let c = parse(&format!("wm_decorations = {text}\n")).unwrap();
            assert_eq!(c.wm_decorations, Some(want));
            assert_eq!(parse(&to_text(&c)).unwrap().wm_decorations, Some(want), "saved and read back");
        }
        assert!(parse("wm_decorations = sometimes\n").is_err());
    }

    #[test]
    fn gamescope_is_a_validated_round_tripping_switch_and_old_fsr_keys_are_read_and_ignored() {
        let c = parse("gamescope = on\ngamescope_output = 1920x1200\n").unwrap();
        assert_eq!((c.gamescope, c.gamescope_output), (Some(true), Some((1920, 1200))));
        assert_eq!(parse(&to_text(&c)).unwrap(), c);
        assert!(parse("gamescope = maybe\n").is_err() && parse("gamescope_output = big\n").is_err());
        // a configuration written before FSR was removed: accepted, even with odd values, and without any effect
        let old = parse("client_exe = /x\nfsr = on\nfsr_internal = 1280x800\nfsr_output = 1920x1200\nfsr_sharpness = 7\n").unwrap();
        assert_eq!(old.gamescope, None, "an old fsr = on does NOT switch gamescope on");
        assert!(parse("fsr = maybe\nfsr_internal = nonsense\nfsr_sharpness = 99\n").is_ok(), "no error for legacy keys");
        let text = to_text(&old);
        assert!(!text.contains("fsr"), "legacy keys are not written back");
    }
}
