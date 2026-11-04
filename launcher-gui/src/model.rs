//! GUI-independent logic of the launcher window. Everything that can be tested without a display lives here.
//!
//! The GUI never invents state: installation, runtime and prefix come from the shared detection in `tsw_launch::discover`
//! (the same code the CLI uses), checks come from `tsw_launch::doctor`/`prefix`, and the game is started through the CLI
//! with the same configuration file, so GUI, CLI and prefix manager cannot disagree.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use tsw_launch::config::{self, Config};
use tsw_launch::discover::Detected;
use tsw_launch::doctor::{self, Finding, Level};
use tsw_launch::install;
use tsw_launch::prefix;
use tsw_launch::runner::Runner;

use crate::i18n::Msg;
use crate::messages;

/// Shown in the advanced view as the patcher status. Deliberately not a result: our own patcher does not exist yet.
pub fn patch_notice() -> Msg {
    Msg::plain(
        "Patcher: the client patch system is built in but stays switched off until the project signing key exists. No client patch is checked or installed. “Play” relies on the local checks only.",
    )
}

/// The version line of the advanced view: application, patch engine and manifest format (the client patch level is shown by `tsw-launch patch status`).
pub fn version_info() -> Msg {
    Msg::new("TSW-Forever {0} · patch engine {1} · patch manifest format {2}", [tsw_launch::project::APP_VERSION.to_string(), tsw_patch_version(), tsw_launch::project::manifest_format().to_string()])
}

fn tsw_patch_version() -> String {
    tsw_launch::project::engine_version().to_string()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Ok,
    Warn,
    Fail,
}

impl Kind {
    pub fn level(self) -> i32 {
        match self {
            Kind::Ok => 0,
            Kind::Warn => 1,
            Kind::Fail => 2,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub kind: Kind,
    pub text: Msg,
    pub hint: Msg,
}

#[derive(Clone, Debug)]
pub struct Preparation {
    pub rows: Vec<Row>,
    /// "Spielen" may be enabled: no failing local check. Warnings do not block.
    pub can_play: bool,
}

/// State of the prefix the detection chose.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PrefixState {
    /// No Wine prefix there yet.
    Missing,
    /// Exists, but the manager's components or manifest are missing/outdated.
    NeedsRefresh,
    Ready,
    /// Unusable (for example corrupt window metrics); never repaired automatically.
    Broken(Msg),
}

pub fn prefix_state(d: &Detected) -> PrefixState {
    if !d.prefix.join("user.reg").is_file() {
        return PrefixState::Missing;
    }
    let wine = doctor::find_program(Path::new(&d.runner), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from(&d.runner));
    let findings = prefix::check(&d.prefix, prefix::runtime_files_dir(&wine).as_deref());
    if let Some(bad) = findings.iter().find(|f| f.what.contains("window metric") && f.level == Level::Fail) {
        let (what, hint) = messages::finding(bad);
        return PrefixState::Broken(Msg::new("{0} – {1}", [what, hint]));
    }
    if findings.iter().any(|f| f.level != Level::Ok) {
        PrefixState::NeedsRefresh
    } else {
        PrefixState::Ready
    }
}

fn row_from(f: &Finding) -> Row {
    let kind = match f.level {
        Level::Ok => Kind::Ok,
        Level::Warn => Kind::Warn,
        Level::Fail => Kind::Fail,
    };
    let (text, hint) = messages::finding(f);
    Row { kind, text, hint }
}

/// Local, offline checks for the detected installation. Nothing is changed or contacted.
pub fn prepare(d: &Detected) -> Preparation {
    let runner = Runner::from_spec(Some(d.runner.as_str()), Some(d.prefix.clone()));
    let has_display = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut findings = doctor::check_all(&d.client_exe, &runner, Some(&d.prefix), &doctor::default_dirs(), std::env::var_os("PATH").as_deref(), has_display);
    if let Some(dir) = d.client_exe.parent() {
        findings.extend(tsw_launch::shadercache::check(dir, Some(&d.prefix), tsw_launch::shadercache::default_state_file().as_deref()));
    }
    let mut rows: Vec<Row> = Vec::new();
    if d.prefix.join("user.reg").is_file() {
        let wine = doctor::find_program(Path::new(&d.runner), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from(&d.runner));
        findings.extend(prefix::check(&d.prefix, prefix::runtime_files_dir(&wine).as_deref()));
    } else {
        rows.push(Row { kind: Kind::Fail, text: Msg::new("Wine prefix {0} is not set up", [d.prefix.display().to_string()]), hint: Msg::plain("it is created automatically for a managed prefix") });
    }
    let ok = findings.iter().filter(|f| f.level == Level::Ok).count();
    rows.extend(findings.iter().filter(|f| f.level != Level::Ok).map(row_from));
    for (i, n) in d.notes.iter().enumerate() {
        let text = match d.note_codes.get(i) {
            Some((code, args)) => messages::note(code, args, n),
            None => Msg::raw(n.clone()),
        };
        rows.push(Row { kind: Kind::Warn, text, hint: Msg::default() });
    }
    if ok > 0 {
        rows.push(Row { kind: Kind::Ok, text: Msg::new("{0} local checks passed", [ok.to_string()]), hint: Msg::default() });
    }
    let can_play = !rows.iter().any(|r| r.kind == Kind::Fail);
    rows.sort_by_key(|r| match r.kind {
        Kind::Fail => 0,
        Kind::Warn => 1,
        Kind::Ok => 2,
    });
    Preparation { rows, can_play }
}

/// What the quiet status line in the normal view says: (text, 0 normal / 1 hint / 2 problem).
pub fn headline(prep: &Preparation) -> (Msg, i32) {
    if let Some(f) = prep.rows.iter().find(|r| r.kind == Kind::Fail) {
        return (Msg::new("Not ready: {0}. Details are in the advanced view.", [f.text.clone()]), 2);
    }
    if prep.rows.iter().any(|r| r.kind == Kind::Warn) {
        return (Msg::plain("Ready. Notes are in the advanced view."), 1);
    }
    (Msg::plain("Ready."), 0)
}

/// The installation folder: empty = the default [`install::default_base`] (`~/Games/The Secret World`, in a Flatpak as well - the sandbox is granted
/// exactly this folder); a leading `~/` is the home folder. The window shows the default; a typed folder only comes from an older state file.
pub fn install_layout(text: &str, home: Option<&Path>) -> Result<install::Layout, Msg> {
    let t = text.trim();
    let path = if t.is_empty() {
        install::default_base(home)
    } else if let Some(rest) = t.strip_prefix("~/") {
        home.map(|h| h.join(rest))
    } else {
        Some(PathBuf::from(t))
    };
    let path = path.ok_or_else(|| Msg::plain("The home folder is unknown: enter the full installation folder"))?;
    install::Layout::new(&path, home).map_err(|e| Msg::new("Installation folder: {0}", [e.to_string()]))
}

/// What the INSTALLATION card says and offers: `(status, action label, action possible)`. Read-only inspection of `layout`.
/// `installer_sha`: the cached SHA-256 of the installer (computed in the background; `None` while it is still being computed).
pub fn install_status(layout: &install::Layout, state: &install::State, import_given: bool, installer_sha: Option<&str>) -> (Msg, Msg, bool) {
    use install::{InstallError, Next, Stage};
    let a = install::assess(layout, state, installer_sha);
    let blocked = |m: String| Msg::new("Not possible yet: {0}", [m]);
    match (a.stage, a.next) {
        (Stage::Installed { client }, _) => (Msg::new("Installed: {0}", [client.display().to_string()]), Msg::plain("Use this installation"), true),
        (Stage::Empty, _) => (Msg::plain("No installation yet. The folders below the chosen installation folder will be created first."), Msg::plain("Create folders"), true),
        (stage, Next::GetInstaller) => {
            let head = match stage {
                Stage::PartialDownload { bytes } => Msg::new("The download was interrupted after {0} bytes and will be resumed.", [bytes.to_string()]),
                _ => Msg::plain("The folders are ready. Next: the original installer."),
            };
            if import_given {
                return (head, Msg::plain("Import installer file"), true);
            }
            match install::download_gate(&install::OFFICIAL, state) {
                Ok(_) => (head, Msg::plain("Download installer"), true),
                Err(InstallError::Blocked(m)) => (Msg::new("{0} {1}", [head, blocked(m)]), Msg::plain("Download installer"), false),
                Err(e) => (Msg::new("{0} {1}", [head, blocked(e.to_string())]), Msg::plain("Download installer"), false),
            }
        }
        (Stage::InstallerReady { checksum }, _) => (
            match checksum {
                install::Checksum::Verified => Msg::plain("The installer is in the cache and its checksum matches. Next: the Wine prefix."),
                install::Checksum::Pending => Msg::plain("The installer is in the cache; its fingerprint is being computed … Next: the Wine prefix."),
                install::Checksum::Unverified => Msg::new("The installer is in the cache, but it is not known to be genuine (SHA-256 {0}). Next: the Wine prefix.", [installer_sha.unwrap_or("").to_string()]),
            },
            Msg::plain("Create the Wine prefix"),
            true,
        ),
        (_, Next::RunPatcher) => (Msg::plain("Only the original patcher is installed. Run it to fetch the game data; this window continues by itself when it reports that nothing is left to fetch."), Msg::plain("Run the original patcher"), true),
        (_, Next::RunInstaller) => match installer_sha {
            None => (Msg::plain("Checking the installer fingerprint …"), Msg::plain("Run the original installer"), false),
            Some(sha) => match install::run_gate(layout, state, &install::OFFICIAL, sha) {
                Ok(()) => (
                    Msg::new("{0} {1}", [Msg::plain("Please use the suggested default settings while installing The Secret World and do not change the installation path in particular. The suggested Windows path is inside the Wine/Proton environment managed by our launcher."), Msg::plain("When the installer is finished, press the button again.")]),
                    Msg::plain("Run the original installer"),
                    true,
                ),
                Err(e) => (blocked(e.to_string()), Msg::plain("Run the original installer"), false),
            },
        },
        (_, _) => (Msg::plain("Choose the installation folder."), Msg::plain("Create folders"), true),
    }
}

/// What the window edits. Overrides are empty for "automatic".
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    pub skip_intro: bool,
    pub client_override: String,
    pub runner_override: String,
    pub prefix_override: String,
    pub virtual_desktop: bool,
    pub desktop_size: String,
    /// `auto` or a language code (`de`, `en`, …).
    pub language: String,
    /// Colour theme id (`templars` = default).
    pub theme: String,
    /// Frame-rate limit chosen with the slider (30..=240) and the separate "unlimited" switch; the number is kept while unlimited is on.
    pub fps: u32,
    pub fps_unlimited: bool,
    /// Show DXVK's FPS counter in the game (to verify the limit).
    pub fps_hud: bool,
    pub game_resolution: Option<(u32, u32)>,
    /// Optional plain gamescope window (off by default; the normal start needs no gamescope).
    pub gamescope: bool,
    /// Folder with the DLLs of the compiler profile (empty = not set).
    pub native_dir: String,
    /// Diagnostic logging (off by default) and its level.
    pub diag_on: bool,
    pub diag_detailed: bool,
}

impl Settings {
    pub fn from_config(c: &Config) -> Settings {
        let (w, h) = c.virtual_desktop.or(c.resolution).unwrap_or((1920, 1200));
        Settings {
            skip_intro: c.skip_intro.unwrap_or(false),
            client_override: c.client_exe.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            runner_override: c.runner.clone().unwrap_or_default(),
            prefix_override: c.prefix.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            virtual_desktop: c.virtual_desktop.is_some(),
            desktop_size: format!("{w}x{h}"),
            language: c.language.clone().unwrap_or_else(|| "auto".into()),
            theme: c.theme.clone().unwrap_or_else(|| "templars".into()),
            fps: match c.fps_limit {
                Some(tsw_launch::fps::FpsLimit::Cap(n)) => n,
                _ => tsw_launch::fps::DEFAULT_FPS,
            },
            fps_unlimited: c.fps_limit == Some(tsw_launch::fps::FpsLimit::Unlimited),
            fps_hud: c.fps_hud.unwrap_or(false),
            game_resolution: c.game_resolution,
            gamescope: c.gamescope.unwrap_or(false),
            native_dir: c.native_dir.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
            diag_on: c.diagnostics.is_some_and(|d| d.is_on()),
            diag_detailed: c.diagnostics == Some(tsw_launch::diaglog::DiagLevel::Detailed),
        }
    }

    /// Write these settings into `c`; keys the window does not edit (environment, resolution, …) stay untouched.
    /// Start options the proven start path needs (`-noconsole`, DX11) are set once if absent.
    pub fn apply(&self, c: &mut Config) -> Result<(), Msg> {
        let opt = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        c.client_exe = opt(&self.client_override).map(PathBuf::from);
        c.runner = opt(&self.runner_override);
        c.prefix = opt(&self.prefix_override).map(PathBuf::from);
        c.skip_intro = Some(self.skip_intro);
        c.language = Some(self.language.clone()).filter(|l| l != "auto");
        c.theme = Some(self.theme.clone()).filter(|t| t != "templars");
        c.fps_limit = Some(if self.fps_unlimited { tsw_launch::fps::FpsLimit::Unlimited } else { tsw_launch::fps::FpsLimit::Cap(self.fps.clamp(tsw_launch::fps::MIN_FPS, tsw_launch::fps::MAX_FPS)) });
        c.fps_hud = Some(self.fps_hud).filter(|h| *h);
        c.game_resolution = self.game_resolution;
        if self.game_resolution.is_some() {
            c.game_managed = false;
        }
        c.native_dir = opt(&self.native_dir).map(PathBuf::from);
        c.gamescope = self.gamescope.then_some(true);
        c.diagnostics = Some(match (self.diag_on, self.diag_detailed) {
            (false, _) => tsw_launch::diaglog::DiagLevel::Off,
            (true, false) => tsw_launch::diaglog::DiagLevel::Basic,
            (true, true) => tsw_launch::diaglog::DiagLevel::Detailed,
        })
        .filter(|d| d.is_on());
        c.virtual_desktop =
            if self.virtual_desktop { Some(config::parse_size(&self.desktop_size).ok_or_else(|| Msg::plain("Size of the virtual desktop: use the form 1920x1200 (320 to 16384)"))?) } else { None };
        c.no_console.get_or_insert(true);
        c.dx11.get_or_insert(true);
        if c.game_resolution.is_none() && !c.game_managed && c.gamescope == Some(true) && c.virtual_desktop.is_some() {
            return Err(Msg::plain("Gamescope cannot be combined with the virtual desktop: turn one of them off"));
        }
        Ok(())
    }
}

/// The launcher's normal window size (logical pixels) and the least it is ever shrunk to.
pub const WINDOW_SIZE: (f32, f32) = (1000.0, 660.0);
pub const WINDOW_MIN: (f32, f32) = (640.0, 480.0);
/// Space kept free around the window on a small display (task bars, title decorations of other windows).
const SCREEN_MARGIN: (f32, f32) = (48.0, 96.0);

/// The fixed window size for a display of `screen` logical pixels (`None`: unknown → the normal size). The normal size is
/// kept whenever it fits; on a smaller display the window shrinks (never below [`WINDOW_MIN`]) and the advanced view
/// scrolls inside it. Pure function, so every display size can be tested.
pub fn fit_window_size(screen: Option<(f32, f32)>) -> (f32, f32) {
    let Some((sw, sh)) = screen.filter(|(w, h)| *w > 0.0 && *h > 0.0) else {
        return WINDOW_SIZE;
    };
    let fit = |normal: f32, avail: f32, min: f32| normal.min(avail).max(min);
    (fit(WINDOW_SIZE.0, sw - SCREEN_MARGIN.0, WINDOW_MIN.0), fit(WINDOW_SIZE.1, sh - SCREEN_MARGIN.1, WINDOW_MIN.1))
}

/// The CLI next to this executable (same directory), else `tsw-launch` from `PATH`.
pub fn cli_binary() -> PathBuf {
    let name = if cfg!(windows) { "tsw-launch.exe" } else { "tsw-launch" };
    std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join(name))).filter(|p| p.is_file()).unwrap_or_else(|| PathBuf::from(name))
}

/// Start the game for the window and keep watching it: the CLI stays alive until the client has ended (inside a Flatpak the sandbox would otherwise end
/// with the CLI and take the client with it). The child is returned after its "started" line; the window minimizes, waits for the child and closes
/// when the game ends - for any reason, a crash included. A start that fails returns the readable message instead (the window stays open).
pub fn start_game_child(cli: &Path, config_file: &Path) -> Result<Option<std::process::Child>, Msg> {
    use std::io::BufRead;
    let mut child = Command::new(cli)
        .arg("--config")
        .arg(config_file)
        .arg("--detach")
        .arg("--stay-alive")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| Msg::new("The start command could not be run ({0}): {1}", [cli.display().to_string(), e.to_string()]))?;
    let mut seen = String::new();
    if let Some(err) = child.stderr.take() {
        for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
            seen.push_str(&line);
            seen.push('\n');
            if line.contains("tsw-launch: started") {
                return Ok(Some(child));
            }
        }
    }
    let code = child.wait().ok().and_then(|s| s.code());
    Err(summarize_failure(&seen, code))
}

/// A short message from the CLI's stderr: the first failing check with its hint, else the last remark. The CLI speaks
/// English, so its detail text is inserted as it is (technical wording is never rewritten).
pub fn summarize_failure(stderr: &str, code: Option<i32>) -> Msg {
    let stderr = &tsw_launch::secrets::mask_text(stderr); // nothing from a child process is shown unfiltered
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    if let Some(i) = lines.iter().position(|l| l.starts_with("[FAIL")) {
        let what = lines[i].trim_start_matches("[FAIL ]").trim_start_matches("[FAIL]").trim();
        let hint = lines.get(i + 1).filter(|l| l.starts_with("->")).map(|l| l.trim_start_matches("->").trim());
        return match hint {
            Some(h) => Msg::new("Starting is not possible: {0} ({1})", [what, h]),
            None => Msg::new("Starting is not possible: {0}", [what]),
        };
    }
    match lines.last() {
        Some(l) => Msg::new("Starting failed: {0}", [l.trim_start_matches("tsw-launch:").trim()]),
        None => Msg::new("Starting failed (exit code {0}).", [code.map_or("?".to_string(), |c| c.to_string())]),
    }
}

/// Optional user-supplied background image: `background.png` or `background.jpg` next to the configuration file.
/// No artwork is built into the program; the original game's artwork is never copied.
pub fn background_image(config_file: &Path) -> Option<PathBuf> {
    let dir = config_file.parent()?;
    ["background.png", "background.jpg"].iter().map(|n| dir.join(n)).find(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Catalog;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-gui-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn detected(d: &Path) -> Detected {
        Detected {
            client_exe: d.join("missing.exe"),
            runner: d.join("wine").display().to_string(),
            prefix: d.join("pfx"),
            managed_prefix: true,
            notes: vec!["Mehrere Laufzeiten gefunden, verwendet wird X.".into()],
            note_codes: vec![("discover.many_runtimes", vec!["X".into()])],
        }
    }

    #[test]
    fn an_unusable_installation_blocks_play_and_failures_come_first() {
        let d = tmp("prep");
        let p = prepare(&detected(&d));
        assert!(!p.can_play);
        let levels: Vec<_> = p.rows.iter().map(|r| r.kind.level()).collect();
        let mut sorted = levels.clone();
        sorted.sort_by_key(|k| match k {
            2 => 0,
            1 => 1,
            _ => 2,
        });
        assert_eq!(levels, sorted);
        let en = Catalog::new("en");
        assert!(p.rows.iter().any(|r| en.render(&r.text) == "Several runtimes were found; X is used."), "detection notes are shown, not hidden");
        assert!(p.rows.iter().any(|r| Catalog::new("de").render(&r.text) == "Mehrere Laufzeiten gefunden, verwendet wird X."), "and translated");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_quiet_headline_names_the_first_problem_and_stays_short_when_fine() {
        let (en, de) = (Catalog::new("en"), Catalog::new("de"));
        let row = |kind, t: &str| Row { kind, text: Msg::raw(t), hint: Msg::default() };
        let ok = Preparation { rows: vec![row(Kind::Ok, "x")], can_play: true };
        assert_eq!((en.render(&headline(&ok).0), headline(&ok).1), ("Ready.".to_string(), 0));
        assert_eq!(de.render(&headline(&ok).0), "Bereit.");
        let warn = Preparation { rows: vec![row(Kind::Warn, "w")], can_play: true };
        assert_eq!(headline(&warn).1, 1);
        let bad = Preparation { rows: vec![row(Kind::Fail, "client missing")], can_play: false };
        let (t, k) = headline(&bad);
        assert_eq!(en.render(&t), "Not ready: client missing. Details are in the advanced view.");
        assert!(de.render(&t).starts_with("Nicht bereit: client missing.") && de.render(&t).contains("erweiterten Ansicht") && k == 2);
        assert!(de.render(&patch_notice()).contains("Signaturschlüssel") && en.render(&patch_notice()).contains("signing key"));
        let v = version_info();
        assert!(en.render(&v).starts_with(&format!("TSW-Forever {} · patch engine ", env!("CARGO_PKG_VERSION"))), "{}", en.render(&v));
        assert!(de.render(&v).contains("Patch-Engine") && de.render(&v).contains("Manifestformat"));
        assert_eq!(env!("CARGO_PKG_VERSION"), tsw_launch::project::APP_VERSION, "GUI and CLI report one application version");
    }

    #[test]
    fn the_fixed_window_fits_every_display_size_and_never_exceeds_the_normal_size() {
        assert_eq!(fit_window_size(None), WINDOW_SIZE);
        assert_eq!(fit_window_size(Some((1920.0, 1200.0))), WINDOW_SIZE, "a normal display keeps the normal size");
        assert_eq!(fit_window_size(Some((3840.0, 2160.0))), WINDOW_SIZE, "a large display does not enlarge it");
        assert_eq!(fit_window_size(Some((1366.0, 768.0))), (1000.0, 660.0), "768 px high still fits 660 + margin");
        assert_eq!(fit_window_size(Some((1280.0, 720.0))), (1000.0, 624.0), "720 px: the height shrinks to leave the margin");
        assert_eq!(fit_window_size(Some((960.0, 540.0))), (912.0, 480.0), "HiDPI 1920x1080 at 2x: width and height shrink; height stops at the minimum");
        assert_eq!(fit_window_size(Some((320.0, 240.0))), WINDOW_MIN, "never below the minimum");
        assert_eq!(fit_window_size(Some((0.0, 0.0))), WINDOW_SIZE, "a bogus size is ignored");
        for (w, h) in [(1024.0, 600.0), (1280.0, 800.0), (800.0, 600.0), (2560.0, 1440.0)] {
            let (fw, fh) = fit_window_size(Some((w, h)));
            assert!((WINDOW_MIN.0..=WINDOW_SIZE.0).contains(&fw) && (WINDOW_MIN.1..=WINDOW_SIZE.1).contains(&fh));
        }
    }

    #[test]
    fn prefix_state_tells_missing_from_ready_without_touching_anything() {
        let d = tmp("pstate");
        let det = detected(&d);
        assert_eq!(prefix_state(&det), PrefixState::Missing);
        std::fs::create_dir_all(&det.prefix).unwrap();
        std::fs::write(det.prefix.join("user.reg"), "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"5434094\"\n").unwrap();
        assert!(matches!(prefix_state(&det), PrefixState::Broken(m) if Catalog::new("en").render(&m).contains("window metric")));
        std::fs::write(det.prefix.join("user.reg"), "[Control Panel\\\\Desktop\\\\WindowMetrics] 1\n\"CaptionHeight\"=\"25\"\n\"MenuHeight\"=\"18\"\n").unwrap();
        assert_eq!(prefix_state(&det), PrefixState::NeedsRefresh, "no manifest/components yet");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn settings_keep_foreign_keys_set_proven_start_options_and_validate_the_size() {
        let mut c = config::parse("resolution = 1280x720\nenv.FOO = bar\nsuppress_driver_warning = true\n").unwrap();
        let mut s = Settings::from_config(&c);
        assert!(!s.skip_intro && s.client_override.is_empty() && s.runner_override.is_empty() && s.prefix_override.is_empty());
        s.skip_intro = true;
        s.apply(&mut c).unwrap();
        assert_eq!((c.skip_intro, c.no_console, c.dx11), (Some(true), Some(true), Some(true)));
        assert_eq!((c.resolution, c.client_exe.clone(), c.runner.clone()), (Some((1280, 720)), None, None), "automatic stays automatic");
        assert_eq!(c.env, vec![("FOO".to_string(), "bar".to_string())]);
        s.virtual_desktop = true;
        s.desktop_size = "wide".into();
        let err = s.apply(&mut c).unwrap_err();
        assert!(Catalog::new("de").render(&err).contains("virtuellen Desktops") && Catalog::new("en").render(&err).contains("virtual desktop"));
        s.desktop_size = "2560x1440".into();
        s.apply(&mut c).unwrap();
        assert_eq!(c.virtual_desktop, Some((2560, 1440)));
        c.no_console = Some(false);
        s.apply(&mut c).unwrap();
        assert_eq!(c.no_console, Some(false), "an explicit choice is not overwritten");
        assert_eq!(c.language, None, "automatic language is stored as 'not set'");
        s.language = "de".into();
        s.apply(&mut c).unwrap();
        assert_eq!(c.language.as_deref(), Some("de"));
        assert_eq!(Settings::from_config(&c).language, "de");
        s.language = "auto".into();
        s.apply(&mut c).unwrap();
        assert_eq!(c.language, None);
    }

    fn fake_cli(dir: &Path, body: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let script = dir.join("fake-cli");
        std::fs::write(&script, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        script
    }

    /// `start_game` for a script that this test has just written. Linux refuses to execute a file while any process holds
    /// it open for writing (ETXTBSY), and a parallel test that forks at that very moment holds a copy of the descriptor
    /// until its own exec. That is a property of parallel tests, not of the launcher, so only the tests retry.
    fn start_fresh_script(cli: &Path, config: &Path) -> Result<Option<std::process::Child>, Msg> {
        for _ in 0..100 {
            match start_game_child(cli, config) {
                Err(e) if e.args.iter().any(|a| matches!(a, crate::i18n::Arg::Text(t) if t.contains("Text file busy"))) => std::thread::sleep(std::time::Duration::from_millis(20)),
                other => return other,
            }
        }
        start_game_child(cli, config)
    }

    #[test]
    fn the_game_is_started_with_the_saved_config_and_the_child_stays_for_the_window_to_watch() {
        let d = tmp("start");
        let cli = fake_cli(&d, "printf '%s' \"$*\" > \"$(dirname \"$0\")/args.txt\"; echo 'tsw-launch: started (pid 1); the client keeps running independently' >&2; sleep 2; exit 0");
        let mut child = start_fresh_script(&cli, Path::new("/cfg/file")).unwrap().expect("the child is handed to the window");
        assert!(child.try_wait().unwrap().is_none(), "the CLI stays alive while the game runs (a Flatpak sandbox ends with it)");
        assert_eq!(std::fs::read_to_string(d.join("args.txt")).unwrap(), "--config /cfg/file --detach --stay-alive");
        assert!(child.wait().unwrap().success(), "the window sees the end of the game through the child");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn an_immediate_start_failure_keeps_the_launcher_open_with_a_readable_message() {
        let d = tmp("startfail");
        let cli =
            fake_cli(&d, "echo '[FAIL ] wine program wine not found' >&2; echo \"        -> use --runner /path/to/wine\" >&2; echo 'tsw-launch: a start is not possible (see FAIL above)' >&2; exit 3");
        let (en, de) = (Catalog::new("en"), Catalog::new("de"));
        let err = start_fresh_script(&cli, Path::new("/cfg")).unwrap_err();
        assert!(en.render(&err).starts_with("Starting is not possible: wine program wine not found"), "{err:?}");
        assert!(de.render(&err).starts_with("Der Start ist nicht möglich: wine program wine not found"), "German frame, technical detail untouched");
        assert!(en.render(&err).contains("use --runner"));
        assert!(de.render(&start_game_child(&d.join("does-not-exist"), Path::new("/cfg")).unwrap_err()).contains("konnte nicht ausgeführt werden"));
        assert!(en.render(&summarize_failure("", Some(7))).contains("exit code 7") && de.render(&summarize_failure("", Some(7))).contains("Exit-Code 7"));
        assert!(en.render(&summarize_failure("tsw-launch: boom\n", Some(1))).ends_with("boom"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_background_image_is_optional_and_found_next_to_the_config() {
        let d = tmp("bg");
        let cfg = d.join("config");
        assert_eq!(background_image(&cfg), None);
        std::fs::write(d.join("background.jpg"), b"x").unwrap();
        assert_eq!(background_image(&cfg), Some(d.join("background.jpg")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_frame_rate_setting_defaults_to_60_is_free_between_30_and_240_and_can_be_unlimited() {
        use tsw_launch::fps::FpsLimit;
        let s = Settings::from_config(&Config::default());
        assert_eq!((s.fps, s.fps_unlimited, s.fps_hud), (60, false, false), "default 60 FPS, a default and not a rule");
        let mut c = Config::default();
        Settings { fps: 144, ..s.clone() }.apply(&mut c).unwrap();
        assert_eq!(c.fps_limit, Some(FpsLimit::Cap(144)));
        assert_eq!(Settings::from_config(&c).fps, 144);
        Settings { fps: 1000, ..s.clone() }.apply(&mut c).unwrap();
        assert_eq!(c.fps_limit, Some(FpsLimit::Cap(240)), "an out-of-range slider value is clamped, never written");
        Settings { fps: 30, fps_unlimited: true, fps_hud: true, ..s.clone() }.apply(&mut c).unwrap();
        assert_eq!((c.fps_limit, c.fps_hud), (Some(FpsLimit::Unlimited), Some(true)));
        let back = Settings::from_config(&c);
        assert!(back.fps_unlimited && back.fps_hud, "unlimited and the counter survive a restart");
        Settings { fps_hud: false, ..s }.apply(&mut c).unwrap();
        assert_eq!(c.fps_hud, None, "the counter is only stored when on");
        let text = config::to_text(&c);
        assert!(text.contains("fps_limit = "), "{text}");
    }

    #[test]
    fn diagnostics_are_off_by_default_and_the_profile_folder_is_kept() {
        use tsw_launch::diaglog::DiagLevel;
        let s = Settings::from_config(&Config::default());
        assert!(!s.diag_on && !s.diag_detailed && s.native_dir.is_empty());
        let mut c = Config::default();
        Settings { diag_on: true, native_dir: " /some dir/dlls ".into(), ..s.clone() }.apply(&mut c).unwrap();
        assert_eq!((c.diagnostics, c.native_dir.as_deref().map(|p| p.to_str().unwrap())), (Some(DiagLevel::Basic), Some("/some dir/dlls")));
        Settings { diag_on: true, diag_detailed: true, ..s.clone() }.apply(&mut c).unwrap();
        assert_eq!(c.diagnostics, Some(DiagLevel::Detailed));
        assert!(Settings::from_config(&c).diag_detailed);
        Settings { diag_on: false, diag_detailed: true, ..s }.apply(&mut c).unwrap();
        assert_eq!(c.diagnostics, None, "off stores nothing");
        assert_eq!(c.native_dir, None);
    }

    #[test]
    fn gamescope_is_off_by_default_stored_when_on_and_old_fsr_settings_do_nothing() {
        let mut c = config::parse("client_exe = /x\n").unwrap();
        let mut s = Settings::from_config(&c);
        assert!(!s.gamescope, "off by default");
        s.apply(&mut c).unwrap();
        assert_eq!(c.gamescope, None, "nothing is written while off");
        s.gamescope = true;
        s.apply(&mut c).unwrap();
        assert_eq!(c.gamescope, Some(true));
        assert!(Settings::from_config(&c).gamescope);
        // a configuration from the FSR time: read without error, no effect, not written back
        let old = config::parse("client_exe = /x\nfsr = on\nfsr_internal = 1280x800\nfsr_output = 1920x1200\nfsr_sharpness = 9\n").unwrap();
        let mut so = Settings::from_config(&old);
        assert!(!so.gamescope, "an old fsr = on does not switch gamescope on");
        let mut c2 = old.clone();
        so.apply(&mut c2).unwrap();
        assert!(!config::to_text(&c2).contains("fsr"), "legacy keys vanish on the next save");
        // not combinable with the virtual desktop
        so.gamescope = true;
        let err = Settings { virtual_desktop: true, desktop_size: "1920x1200".into(), ..so }.apply(&mut c2).unwrap_err();
        assert!(Catalog::new("en").render(&err).contains("virtual desktop") && Catalog::new("de").render(&err).contains("virtuellen Desktop"), "{}", Catalog::new("de").render(&err));
    }

    #[test]
    fn the_installation_card_derives_its_text_and_button_from_the_files_and_the_users_decisions() {
        let (en, de) = (Catalog::new("en"), Catalog::new("de"));
        let home = tmp("inst-home");
        // folder input: empty = default below the home folder, ~/ expands, unsafe input is a readable message
        let l = install_layout("", Some(&home)).unwrap();
        assert_eq!(l.base, home.join("Games").join("The Secret World"), "the same default on the host and inside the Flatpak");
        assert_eq!(install_layout("~/tsw", Some(&home)).unwrap().base, home.join("tsw"));
        assert!(en.render(&install_layout("/usr/tsw", Some(&home)).unwrap_err()).starts_with("Installation folder:"));
        assert!(de.render(&install_layout("/usr/tsw", Some(&home)).unwrap_err()).starts_with("Installationsordner:"));
        assert!(install_layout("", None).is_err());

        let st = install::State::default();
        let (m, label, on) = install_status(&l, &st, false, None);
        assert!(en.render(&m).starts_with("No installation yet") && en.render(&label) == "Create folders" && on);
        install::prepare(&l).unwrap();
        // no consent, no checksum: the button is off and the text says why
        let (m, label, on) = install_status(&l, &st, false, None);
        assert!(!on && en.render(&m).contains("consent") && en.render(&label) == "Download installer", "{}", en.render(&m));
        let consent = install::State { download_consent: Some(install::OFFICIAL.url.into()), ..st.clone() };
        let (m, _, on) = install_status(&l, &consent, false, None);
        assert!(on, "consent is enough because the shipped checksum is pinned: {}", en.render(&m));
        let pinned = install::State { pinned_sha256: Some("ab".repeat(32)), ..consent };
        assert!(install_status(&l, &pinned, false, None).2, "consent + a checksum pinned by the user enables the download as well");
        let (m, label, on) = install_status(&l, &st, true, None);
        assert!(on && en.render(&label) == "Import installer file", "{}", en.render(&m));
        // installer cached, unverified
        std::fs::write(l.installer(), b"exe").unwrap();
        let sha = tsw_launch::sha256::sha256_hex(b"exe");
        let (m, label, on) = install_status(&l, &st, false, Some(&sha));
        assert!(
            on && en.render(&label) == "Create the Wine prefix"
                && en.render(&m).contains("not known to be genuine")
                && en.render(&m).contains(&sha)
                && de.render(&m).contains("nicht als echt bekannt")
        );
        assert!(en.render(&install_status(&l, &st, false, None).0).contains("being computed"), "the fingerprint is computed in the background, the card does not wait");
        // prefix ready: running needs the confirmation of THIS file's fingerprint, and the mandatory notice is shown before the start
        std::fs::write(l.prefix().join("user.reg"), b"x").unwrap();
        std::fs::write(l.prefix().join(tsw_launch::prefix::MANIFEST), b"x").unwrap();
        assert!(!install_status(&l, &st, false, None).2, "no fingerprint yet");
        assert!(!install_status(&l, &st, false, Some(&sha)).2, "unknown file, no confirmation");
        let confirmed = install::State { confirmed_sha256: Some(sha.clone()), ..st.clone() };
        let (m, _, on) = install_status(&l, &confirmed, false, Some(&sha));
        assert!(on && en.render(&m).starts_with(install::INSTALLER_NOTICE), "the notice text equals the library constant");
        assert!(de
            .render(&m)
            .starts_with("Bitte verwende während der Installation von The Secret World die vorgeschlagenen Standardeinstellungen und ändere insbesondere den Installationspfad nicht."));
        assert!(!install_status(&l, &confirmed, false, Some(&tsw_launch::sha256::sha256_hex(b"other file"))).2, "another file does not inherit the confirmation");
        // installed (inside the prefix, with its data)
        let exe = l.prefix().join("drive_c/Program Files (x86)/Funcom/The Secret World").join(install::CLIENT_EXE);
        std::fs::create_dir_all(exe.parent().unwrap().join("RDB")).unwrap();
        std::fs::write(exe.parent().unwrap().join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        let (m, label, on) = install_status(&l, &st, false, None);
        assert!(on && en.render(&m).starts_with("Installed:") && de.render(&label) == "Diese Installation verwenden");
        let _ = std::fs::remove_dir_all(&home);
    }
}
