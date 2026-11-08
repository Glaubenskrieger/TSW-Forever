//! Graphical launcher: a quiet window with "Play", the intro option and a status line. The game starts only when
//! "Play" is clicked. Installation, runtime and prefix are detected automatically (shared with the CLI); the CLI
//! `tsw-launch` stays the single start path and remains available for tests and diagnosis.
//!
//! Everything the window shows is derived from [`State`] by [`render`], in the current language. Messages are kept as
//! [`i18n::Msg`] values (id + arguments), not as finished text, so changing the language redraws the whole window
//! without a restart.
// Windows: a normal GUI program without a console window (the command-line tool `tsw-launch.exe` keeps its console; diagnostics go to log files).
#![cfg_attr(all(windows, not(test)), windows_subsystem = "windows")]

mod i18n;
mod messages;
mod model;
mod portal;
mod theme;
#[cfg(test)]
mod ui_tests;

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use slint::{CloseRequestResponse, ComponentHandle, ModelRc, SharedString, VecModel};
use tsw_launch::config::{self, Config};
use tsw_launch::discover::{self, Detected, Env};
use tsw_launch::doctor;
use tsw_launch::install;
use tsw_launch::prefix;
use tsw_launch::progress::{Outcome, Phase, Progress, Sink, Update};

use i18n::{Arg, Catalog, Msg};
use model::{PrefixState, Settings};

slint::include_modules!();

/// Application id of the launcher: Wayland `app_id`, X11 `WM_CLASS`, desktop entry name `<APP_ID>.desktop`, icon name, Flatpak id.
/// Provisional reverse-DNS name; changing it means renaming `packaging/linux/<id>.desktop` and the icon files (a test checks all of them).
pub const APP_ID: &str = "org.tswlauncher.Launcher";

/// What the window currently shows, as messages (no finished text).
#[derive(Default)]
struct View {
    status: Option<(Msg, i32)>,
    message: Option<Msg>,
    rows: Vec<model::Row>,
    progress: Option<Progress>,
    /// The detected client executable (its folder holds the game's local shader cache).
    client_exe: Option<PathBuf>,
    /// The detected Wine prefix (holds the compiler profile).
    prefix: Option<PathBuf>,
    /// INSTALLATION card: `(status, action label)`; the enabled flag is set directly on the window.
    install: Option<(Msg, Msg)>,
    /// Main view: no usable installation yet, so the big button installs or continues the installation.
    needs_install: bool,
    /// Main button text in that mode.
    install_label: Option<Msg>,
    /// Nothing is installed, nothing is running and no installer was chosen yet: Windows then offers the three ways to set the game up.
    first_run: bool,
    /// One of Funcom's programs works and has not confirmed completion: the main button is disabled.
    install_waiting: bool,
    /// What the installation waits for (shown under the main button in that mode).
    install_hint: Option<Msg>,
    /// Compact installation folder for the window (`~/Games/The Secret World`); on Windows the folder of the GAME (never the launcher's own data folder).
    location: String,
    /// Windows: the state of the game installation in words (not installed / found but not confirmed complete), and its kind (1 = needs attention).
    install_state: Option<(Msg, i32)>,
    /// Windows: every installation found on this computer (client executables) while none has been chosen yet.
    detected: Vec<PathBuf>,
    /// Windows: Direct3D libraries of the DirectX June 2010 runtime that are missing (file names).
    directx_missing: Vec<String>,
    /// Windows: existing installations the user chose to ignore ("install a new copy").
    ignored: Vec<PathBuf>,
    /// A remark about an earlier layout that is only reported.
    install_notice: Option<Msg>,
}

struct State {
    config_path: PathBuf,
    config: Config,
    catalog: Catalog,
    view: View,
    /// The user's installation decisions (folder, consent, pinned checksum), persisted next to the configuration.
    install: tsw_launch::install::State,
    install_cancel: Arc<AtomicBool>,
    /// The user pressed the main button: after each finished step the next one follows by itself, until the original installer needs the user.
    install_auto: bool,
    /// SHA-256 of the cached installer, computed OFF the UI thread and valid for the file's size and modification time.
    installer_hash: Option<InstallerHash>,
    hash_pending: bool,
    /// What the watcher saw of Funcom's programs and of the patcher's verdict (see `install::gate`).
    evidence: install::Evidence,
    /// A watcher pass is running in the background (one at a time).
    watch_busy: bool,
    /// Funcom programs this window started; kept so that finished ones are collected (no zombies) and so that they are not forgotten.
    funcom_children: Vec<std::process::Child>,
    /// The original patcher THIS window started (only that one may be asked to close, and only after Funcom confirmed the installation).
    patcher_child: Option<std::process::Child>,
    /// Closing was already tried for the current patcher run (one attempt per run).
    close_attempted: bool,
    /// What was seen of the current (or last) run of the original patcher, whoever started it.
    run: install::RunWatch,
    run_active: bool,
    /// The last run ended after the patcher announced that it is done and wrote nothing big any more (see `install::RunWatch`).
    run_ok: bool,
}

#[derive(Clone, Debug)]
struct InstallerHash {
    size: u64,
    modified: Option<std::time::SystemTime>,
    sha256: String,
}
type Shared = Arc<Mutex<State>>;

fn env_var(k: &str) -> Option<String> {
    std::env::var(k).ok()
}

/// The language the window starts in: the saved setting, else the system locale, else English.
fn startup_language(config: &Config, env: &dyn Fn(&str) -> Option<String>) -> &'static str {
    i18n::resolve(config.language.as_deref(), env)
}

/// Index of the language entry in the selector: 0 = automatic, then [`i18n::LANGUAGES`] in order.
pub(crate) fn language_index(setting: Option<&str>) -> i32 {
    match setting {
        None | Some("auto") | Some("") => 0,
        Some(code) => i18n::LANGUAGES.iter().position(|(c, _, _)| *c == code).map_or(0, |i| i as i32 + 1),
    }
}

/// The setting stored for a selector index (`auto` for 0 and for anything out of range).
pub(crate) fn language_setting(index: i32) -> &'static str {
    if index <= 0 {
        return "auto";
    }
    i18n::LANGUAGES.get(index as usize - 1).map_or("auto", |(c, _, _)| *c)
}

/// Label of a phase, as a message id.
fn phase_id(p: Phase) -> &'static str {
    match p {
        Phase::Checking => "Checking",
        Phase::Downloading => "Downloading",
        Phase::Verifying => "Verifying",
        Phase::Installing => "Installing",
    }
}

/// `63 % · 41,2 MB of 65,0 MB · 12,0 MB/s · 3 s left`: only what was really measured, numbers in the language's format.
fn progress_detail(p: &Progress, cat: &Catalog) -> String {
    let mut parts = Vec::new();
    if let Some(pc) = p.percent() {
        parts.push(cat.percent(pc));
    }
    if let (Some(d), Some(t)) = (p.done, p.total) {
        parts.push(cat.render(&Msg::new("{0} of {1}", [cat.bytes(d), cat.bytes(t)])));
    }
    if let Some(r) = p.rate {
        parts.push(format!("{}/s", cat.bytes(r as u64)));
    }
    if let Some(e) = p.eta_secs {
        parts.push(cat.render(&Msg::new("{0} left", [cat.duration(e)])));
    }
    parts.join(" · ")
}

fn sstr(s: String) -> SharedString {
    SharedString::from(s)
}

/// Draw the whole window from the state. Cheap; called after every change.
fn render(ui: &MainWindow, st: &State) {
    let cat = &st.catalog;
    {
        use slint::Model;
        let names = ui.get_resolution_names();
        if names.row_count() > 0 && names.row_data(0).is_some_and(|s| s.as_str() != cat.render(&Msg::plain("Managed by the game (default)"))) {
            let modes: Vec<_> = names.iter().skip(1).filter_map(|s| config::parse_size(&s.replace(" × ", "x"))).collect();
            set_resolution_modes(ui, st, &modes);
        }
    }
    match &st.view.status {
        Some((m, kind)) => {
            ui.set_status_text(sstr(cat.render(m)));
            ui.set_status_kind(*kind);
        }
        None => ui.set_status_text(SharedString::new()),
    }
    ui.set_message(sstr(st.view.message.as_ref().map(|m| cat.render(m)).unwrap_or_default()));
    let rows: Vec<StatusRow> = st.view.rows.iter().map(|r| StatusRow { level: r.kind.level(), text: sstr(cat.render(&r.text)), hint: sstr(cat.render(&r.hint)) }).collect();
    ui.set_rows(ModelRc::new(VecModel::from(rows)));
    match &st.view.progress {
        Some(p) => {
            ui.set_progress_visible(true);
            ui.set_progress_indeterminate(p.fraction().is_none());
            ui.set_progress_fraction(p.fraction().unwrap_or(0.0));
            let title = Msg::new("{0} – {1}", [Arg::Id(phase_id(p.phase).into()), Arg::Id(p.step.clone())]);
            ui.set_progress_title(sstr(cat.render(&title)));
            ui.set_progress_detail(sstr(progress_detail(p, cat)));
        }
        None => ui.set_progress_visible(false),
    }
    if let Some((status, label)) = &st.view.install {
        ui.set_install_status(sstr(cat.render(status)));
        ui.set_install_action_label(sstr(cat.render(label)));
    }
    ui.set_install_mode(st.view.needs_install);
    ui.set_install_location(sstr(st.view.location.clone()));
    ui.set_install_main_label(sstr(st.view.install_label.as_ref().map(|m| cat.render(m)).unwrap_or_default()));
    ui.set_install_notice(sstr(st.view.install_notice.as_ref().map(|m| cat.render(m)).unwrap_or_default()));
    ui.set_install_waiting(st.view.install_waiting);
    ui.set_first_run(st.view.first_run);
    ui.set_ignored_text(sstr(if st.view.ignored.is_empty() {
        String::new()
    } else {
        cat.render(&Msg::new("Ignored existing installation: {0}", [st.view.ignored.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join("; ")]))
    }));
    ui.set_directx_text(sstr(if st.view.directx_missing.is_empty() {
        String::new()
    } else {
        cat.render(&Msg::new(
            "These Direct3D libraries of Microsoft's DirectX End-User Runtimes (June 2010) are missing on this computer: {0}. The original game needs them in their 32-bit form; without them it may not start or may show errors.",
            [st.view.directx_missing.join(", ")],
        ))
    }));
    ui.set_install_state(sstr(st.view.install_state.as_ref().map(|(m, _)| cat.render(m)).unwrap_or_default()));
    ui.set_install_state_kind(st.view.install_state.as_ref().map_or(0, |(_, k)| *k));
    let listed: Vec<slint::SharedString> = st.view.detected.iter().map(|p| p.display().to_string().into()).collect();
    if slint::Model::row_count(&ui.get_detected_installs()) != listed.len() {
        ui.set_detected_index(0);
    }
    ui.set_detected_installs(slint::ModelRc::new(slint::VecModel::from(listed)));
    ui.set_install_hint(sstr(st.view.install_hint.as_ref().map(|m| cat.render(m)).unwrap_or_default()));
    ui.set_patch_status(sstr(cat.render(&model::patch_notice())));
    ui.set_version_info(sstr(cat.render(&model::version_info())));
    ui.set_cache_status(sstr(cache_status_text(st)));
    ui.set_profile_status(sstr(profile_status_text(st)));
    ui.set_diag_level_names(ModelRc::new(VecModel::from(vec![
        SharedString::from(cat.render(&Msg::plain("Basic (launcher events)"))),
        SharedString::from(cat.render(&Msg::plain("Detailed (with game and DXVK output)"))),
    ])));
    let mut names = vec![SharedString::from(cat.render(&Msg::plain("Automatic (system language)")))];
    names.extend(i18n::LANGUAGES.iter().map(|(_, n, _)| SharedString::from(*n)));
    ui.set_language_names(ModelRc::new(VecModel::from(names)));
    ui.set_language_index(language_index(st.config.language.as_deref()));
    // appearance: theme names and the description of the chosen theme, in the current language
    let names: Vec<SharedString> = theme::THEMES.iter().map(|t| SharedString::from(cat.tr(t.name))).collect();
    ui.set_theme_names(ModelRc::new(VecModel::from(names)));
    let current = theme::resolve(st.config.theme.as_deref());
    ui.set_theme_index(theme::index_of(st.config.theme.as_deref()));
    ui.set_theme_description(sstr(cat.tr(current.description).to_string()));
    theme::apply(ui, current);
}

/// Switch the language (window texts and program messages) and redraw.
fn apply_language(ui: &MainWindow, st: &mut State, code: &str) {
    st.catalog = Catalog::new(code);
    i18n::select_language(st.catalog.code());
    render(ui, st);
}

/// Run `f` on the UI thread with the state, then redraw. Callable from any thread.
fn post(weak: &slint::Weak<MainWindow>, state: &Shared, f: impl FnOnce(&MainWindow, &mut State) + Send + 'static) {
    let state = state.clone();
    let _ = weak.upgrade_in_event_loop(move |ui| {
        let mut st = state.lock().unwrap();
        f(&ui, &mut st);
        render(&ui, &st);
    });
}

fn set_status(st: &mut State, msg: Msg, kind: i32) {
    st.view.status = Some((msg, kind));
}

/// Does the configuration name a client that may be used? A stored path of an old configuration does not count while the detection is paused.
fn config_names_client(st: &State) -> bool {
    tsw_launch::winnative::stored_paths_honored() && st.config.client_exe.as_ref().is_some_and(|p| p.is_file())
}

fn explicit(c: &Config) -> Config {
    Config { client_exe: c.client_exe.clone(), runner: c.runner.clone(), prefix: c.prefix.clone(), ..Config::default() }
}

fn wine_of(d: &Detected) -> PathBuf {
    doctor::find_program(Path::new(&d.runner), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from(&d.runner))
}

/// Detect, prepare the managed prefix if needed (with real progress) and run the local checks, off the UI thread.
fn prepare_in_background(ui: &MainWindow, state: &Shared, refresh_prefix: bool) {
    ui.set_busy(true);
    let cfg = state.lock().unwrap().config.clone();
    let weak = ui.as_weak();
    let state = state.clone();
    std::thread::spawn(move || {
        // while the automatic detection is paused (Windows) a stored path of an old configuration and the detection cache are not used
        let honored = tsw_launch::winnative::stored_paths_honored();
        let cfg = if honored { cfg } else { Config { client_exe: None, ..cfg } };
        let cache = if honored { discover::cache_path() } else { None };
        let detected = discover::resolve(&Env::from_process(), &explicit(&cfg), cache.as_deref());
        let d = match detected {
            Ok(d) => d,
            Err(problem) => {
                // no installation yet: the main view offers the installation, so no error text (the advanced view still names the problem)
                let not_installed = matches!(problem, discover::Problem::NoInstall);
                let msg = messages::problem(&problem);
                post(&weak, &state, move |ui, st| {
                    if not_installed {
                        set_status(st, Msg::plain("The game is not installed yet. Agree to the downloads and press the button."), 0);
                        st.view.message = None;
                    } else {
                        set_status(st, msg.clone(), 2);
                        st.view.message = Some(msg);
                    }
                    ui.set_can_play(false);
                    ui.set_busy(false);
                });
                return;
            }
        };
        let (c, r, p) = (d.client_exe.display().to_string(), d.runner.clone(), d.prefix.display().to_string());
        post(&weak, &state, move |ui, st| {
            st.view.client_exe = Some(PathBuf::from(&c));
            st.view.prefix = Some(PathBuf::from(&p));
            ui.set_detected_client(c.into());
            ui.set_detected_runtime(r.into());
            ui.set_detected_prefix(p.into());
        });

        let mut state_of_prefix = model::prefix_state(&d);
        let wants_init = matches!(state_of_prefix, PrefixState::Missing | PrefixState::NeedsRefresh) || refresh_prefix;
        if d.managed_prefix && wants_init {
            let (weak2, state2) = (weak.clone(), state.clone());
            let sink = move |u: Update| {
                post(&weak2, &state2, move |_, st| match u {
                    Update::Progress(p) => st.view.progress = Some(p),
                    Update::Done(Outcome::Finished(_)) => st.view.progress = None,
                    Update::Done(Outcome::Failed(detail)) => {
                        st.view.progress = None;
                        let msg = Msg::new("The Wine prefix could not be prepared: {0}", [detail]);
                        st.view.message = Some(msg.clone());
                        set_status(st, msg, 2);
                    }
                });
            };
            let _ = prefix::init_with(&d.prefix, &wine_of(&d), &sink as &dyn Sink);
            state_of_prefix = model::prefix_state(&d);
        }
        if let PrefixState::Broken(msg) = state_of_prefix {
            post(&weak, &state, move |ui, st| {
                set_status(st, Msg::plain("The Wine prefix is unusable. Details are in the advanced view."), 2);
                st.view.message = Some(msg);
                ui.set_can_play(false);
                ui.set_busy(false);
            });
            return;
        }

        let prep = model::prepare(&d);
        let (line, kind) = model::headline(&prep);
        post(&weak, &state, move |ui, st| {
            st.view.rows = prep.rows;
            set_status(st, line, kind);
            ui.set_can_play(prep.can_play);
            ui.set_busy(false);
        });
    });
}

/// The folder of the detected client (where the game keeps its local shader cache), if a client was detected.
fn game_dir(st: &State) -> Option<PathBuf> {
    st.view.client_exe.as_deref().and_then(Path::parent).map(Path::to_path_buf)
}

/// One line about the local shader cache in the user's language (facts only; see shadercache.rs).
fn cache_status_text(st: &State) -> String {
    let cat = &st.catalog;
    let Some(dir) = game_dir(st) else {
        return cat.render(&Msg::plain("no client found yet"));
    };
    let path = dir.join(tsw_launch::shadercache::LOCAL_CACHE);
    let aside = tsw_launch::shadercache::quarantined(&dir).len();
    let head = match tsw_launch::shadercache::scan_file(&path) {
        Ok(s) if s.vkd3d_count() > 0 => cat.render(&Msg::new("{0} of {1} shader(s) were compiled by Wine's built-in compiler", [s.vkd3d_count().to_string(), s.blobs.len().to_string()])),
        Ok(s) => cat.render(&Msg::new("{0} shader(s), none compiled by Wine's built-in compiler", [s.blobs.len().to_string()])),
        Err(_) if !path.is_file() => cat.render(&Msg::plain("no local shader cache yet")),
        Err(_) => cat.render(&Msg::plain("the local shader cache could not be inspected")),
    };
    if aside > 0 {
        format!("{head} · {}", cat.render(&Msg::new("{0} set aside", [aside.to_string()])))
    } else {
        head
    }
}

/// Is the compiler profile installed in the detected prefix? One plain line.
fn profile_status_text(st: &State) -> String {
    let cat = &st.catalog;
    let Some(prefix) = st.view.prefix.as_deref() else {
        return cat.render(&Msg::plain("no prefix found yet"));
    };
    match prefix::profile_finding(prefix) {
        Some(f) if f.level == tsw_launch::doctor::Level::Ok => cat.render(&Msg::new("Compiler profile {0} is installed.", [f.args.first().cloned().unwrap_or_default()])),
        Some(_) => cat.render(&Msg::plain("No compiler profile installed. The tested configuration uses it (see the hint in the checks).")),
        None => cat.render(&Msg::plain("The prefix is not prepared yet.")),
    }
}

/// Install or remove the compiler profile after the user pressed the button, then check again.
fn profile_action(ui: &MainWindow, state: &Shared, install: bool) {
    let prefix = state.lock().unwrap().view.prefix.clone();
    let dir = ui.get_native_dir().to_string();
    let msg = match prefix {
        None => Msg::plain("No prefix found yet."),
        Some(p) if install => match prefix::install_native(&prefix::PROFILE_D3D_NATIVE_V7, Path::new(dir.trim()), &p) {
            Ok(()) => Msg::plain("The compiler profile was installed."),
            Err(e) => Msg::new("Compiler profile: {0}", [e]),
        },
        Some(p) => match prefix::remove_native(&p) {
            Ok(_) => Msg::plain("The compiler profile was removed; the previous files are back."),
            Err(e) => Msg::new("Compiler profile: {0}", [e]),
        },
    };
    {
        let mut st = state.lock().unwrap();
        st.view.message = Some(msg);
        render(ui, &st);
    }
    prepare_in_background(ui, state, false);
}

/// Open the log folder with the desktop's opener (xdg-open); without one the path is shown.
fn open_logs(ui: &MainWindow, state: &Shared) {
    let mut st = state.lock().unwrap();
    let dir = tsw_launch::diaglog::log_dir(&st.config_path);
    let _ = std::fs::create_dir_all(&dir);
    st.view.message = Some(match std::process::Command::new("xdg-open").arg(&dir).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).spawn() {
        Ok(_) => Msg::new("Log folder: {0}", [dir.display().to_string()]),
        Err(_) => Msg::new("The log folder could not be opened automatically. It is here: {0}", [dir.display().to_string()]),
    });
    render(ui, &st);
}

/// Create the reduced diagnostic package (through the CLI, one implementation) and show where it is.
fn make_bundle(ui: &MainWindow, state: &Shared) {
    let mut st = state.lock().unwrap();
    let mut command = std::process::Command::new(model::cli_binary());
    tsw_launch::winnative::no_window(&mut command);
    let out = command.args(["diag", "bundle", "--config"]).arg(&st.config_path).stdin(std::process::Stdio::null()).output();
    st.view.message = Some(match out {
        Ok(o) if o.status.success() => Msg::new("Diagnostic package written: {0}. Read it before you share it; nothing was sent.", [String::from_utf8_lossy(&o.stdout).trim().to_string()]),
        Ok(o) => Msg::new("The diagnostic package could not be created: {0}", [tsw_launch::secrets::mask_text(String::from_utf8_lossy(&o.stderr).trim())]),
        Err(e) => Msg::new("The diagnostic package could not be created: {0}", [e.to_string()]),
    });
    render(ui, &st);
}

/// Set the shader cache aside (or bring the last one back) after the user pressed the button. Reversible; never deletes.
fn cache_action(ui: &MainWindow, state: &Shared, restore: bool) {
    let mut st = state.lock().unwrap();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let result = match game_dir(&st) {
        None => Err("no client found yet".to_string()),
        Some(dir) if restore => tsw_launch::shadercache::restore(&dir, now),
        Some(dir) => tsw_launch::shadercache::quarantine(&dir, now),
    };
    st.view.message = Some(match result {
        Ok(_) if restore => Msg::plain("The previous shader cache was restored."),
        Ok(_) => Msg::plain("The shader cache was set aside; the game builds a new one at the next start."),
        Err(e) => Msg::new("Shader cache: {0}", [e]),
    });
    render(ui, &st);
}

fn read_settings(ui: &MainWindow, st: &State) -> Settings {
    Settings {
        skip_intro: ui.get_skip_intro(),
        client_override: ui.get_client_override().to_string(),
        runner_override: ui.get_runner_override().to_string(),
        prefix_override: ui.get_prefix_override().to_string(),
        virtual_desktop: ui.get_virtual_desktop(),
        desktop_size: ui.get_desktop_size().to_string(),
        language: st.config.language.clone().unwrap_or_else(|| "auto".into()),
        theme: st.config.theme.clone().unwrap_or_else(|| theme::DEFAULT.into()),
        fps: ui.get_fps_limit().clamp(tsw_launch::fps::MIN_FPS as i32, tsw_launch::fps::MAX_FPS as i32) as u32,
        fps_unlimited: ui.get_fps_unlimited(),
        fps_hud: ui.get_fps_hud(),
        game_resolution: st.config.game_resolution,
        gamescope: ui.get_gamescope_enabled(),
        native_dir: ui.get_native_dir().to_string(),
        diag_on: ui.get_diag_enabled(),
        diag_detailed: ui.get_diag_level_index() == 1,
    }
}

/// Persist the settings. Returns whether the detection inputs changed (then the preparation must run again), or a
/// message for the user.
fn save(ui: &MainWindow, state: &Shared) -> Result<bool, Msg> {
    let mut st = state.lock().unwrap();
    let before = explicit(&st.config);
    let settings = read_settings(ui, &st);
    settings.apply(&mut st.config)?;
    config::save(&st.config_path, &st.config).map_err(|e| Msg::new("Saving failed: {0}", [e.to_string()]))?;
    Ok(explicit(&st.config) != before)
}

/// Fill the selector only with real modes. An unavailable saved choice stays saved and is explicitly marked.
fn set_resolution_modes(ui: &MainWindow, st: &State, modes: &[(u32, u32)]) {
    let modes = tsw_launch::display::normalize(modes.iter().copied());
    let legacy = st.config.game_resolution.is_none() && !st.config.game_managed && (st.config.resolution.is_some() || st.config.virtual_desktop.is_some() || st.config.gamescope == Some(true));
    ui.set_resolution_legacy(legacy);
    let index = st.config.game_resolution.map_or(if legacy { -1 } else { 0 }, |size| modes.iter().position(|m| *m == size).map_or(-1, |i| i as i32 + 1));
    let names: Vec<slint::SharedString> =
        std::iter::once(st.catalog.render(&Msg::plain("Managed by the game (default)"))).chain(modes.iter().map(|(w, h)| format!("{w} × {h}"))).map(Into::into).collect();
    ui.set_resolution_names(std::rc::Rc::new(slint::VecModel::from(names)).into());
    ui.set_resolution_index(index);
    ui.set_resolution_unavailable(index < 0);
}

fn refresh_resolution_modes(ui: &MainWindow, state: &Shared) {
    let (weak, state) = (ui.as_weak(), state.clone());
    let _ = std::thread::Builder::new().name("display-refresh".into()).spawn(move || {
        let modes = tsw_launch::display::available_modes();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                set_resolution_modes(&ui, &state.lock().unwrap(), &modes);
            }
        });
    });
}

/// Fit the fixed window to the display: the default size, but never larger than the screen allows. The window keeps
/// min = max = this size (not resizable); a smaller display gets the advanced view's own scrolling instead of a window
/// that does not fit. HiDPI: Slint sizes are logical, so the physical display size is divided by the scale factor.
fn fit_window(ui: &MainWindow) {
    let scale = ui.window().scale_factor().max(0.5);
    let screen = tsw_launch::display::system_primary_mode().map(|(w, h)| (w as f32 / scale, h as f32 / scale));
    let (w, h) = model::fit_window_size(screen);
    ui.set_win_width(w);
    ui.set_win_height(h);
}

/// Leave the launcher without asking: after the user confirmed, or after the game was started successfully. Only the
/// launcher window goes away; the game is a separate process and is not touched.
fn leave(ui: &MainWindow, may_close: &AtomicBool) {
    may_close.store(true, Ordering::SeqCst);
    let _ = ui.hide();
}

/// Connect every callback of the window to the program. Separate from `main` so that the tests drive the real wiring.
fn wire(ui: &MainWindow, state: &Shared, may_close: &Arc<AtomicBool>) {
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_microsoft_terms_link(move |which| {
            let (weak, state) = (weak.clone(), state.clone());
            std::thread::spawn(move || {
                if let Err(e) = portal::open_microsoft_terms(which) {
                    post(&weak, &state, move |_, st| st.view.message = Some(Msg::new("Could not open the licence link: {0}", [e])));
                }
            });
        });
    }

    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_refresh_display(move || {
            if let Some(ui) = weak.upgrade() {
                refresh_resolution_modes(&ui, &state);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_resolution_selected(move |index| {
            use slint::Model;
            let Some(ui) = weak.upgrade() else {
                return;
            };
            let size = if index == 0 {
                None
            } else {
                let Some(text) = usize::try_from(index).ok().and_then(|i| ui.get_resolution_names().row_data(i)) else {
                    return;
                };
                let Some(size) = config::parse_size(&text.replace(" × ", "x")) else {
                    return;
                };
                Some(size)
            };
            let old = {
                let mut st = state.lock().unwrap();
                let old = (st.config.game_resolution, st.config.game_managed);
                st.config.game_resolution = size;
                st.config.game_managed = index == 0;
                old
            };
            if let Err(e) = save(&ui, &state) {
                let mut st = state.lock().unwrap();
                st.config.game_resolution = old.0;
                st.config.game_managed = old.1;
                st.view.message = Some(e);
                let modes: Vec<_> = ui.get_resolution_names().iter().skip(1).filter_map(|s| config::parse_size(&s.replace(" × ", "x"))).collect();
                set_resolution_modes(&ui, &st, &modes);
                render(&ui, &st);
            } else {
                ui.set_resolution_index(index);
                ui.set_resolution_unavailable(false);
                ui.set_resolution_legacy(false);
            }
        });
    }
    // Closing: our own X button, the window manager (Alt+F4, task bar) and Ctrl+Q all ask first. Only a confirmed exit or the
    // automatic close after a successful start sets the flag; the started game is a separate process and is never touched.
    {
        let (weak, may_close) = (ui.as_weak(), may_close.clone());
        ui.window().on_close_requested(move || {
            if may_close.load(Ordering::SeqCst) {
                return CloseRequestResponse::HideWindow;
            }
            if let Some(ui) = weak.upgrade() {
                ui.set_confirm_exit(true);
            }
            CloseRequestResponse::KeepWindowShown
        });
    }
    {
        let (weak, may_close, state) = (ui.as_weak(), may_close.clone(), state.clone());
        ui.on_close_window(move || {
            let st = state.lock().unwrap();
            st.install_cancel.store(true, Ordering::SeqCst);
            drop(st);
            if let Some(ui) = weak.upgrade() {
                leave(&ui, &may_close);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_settings_changed(move || {
            let Some(ui) = weak.upgrade() else { return };
            match save(&ui, &state) {
                Ok(true) => prepare_in_background(&ui, &state, false),
                Ok(false) => {}
                Err(e) => {
                    let mut st = state.lock().unwrap();
                    st.view.message = Some(e);
                    render(&ui, &st);
                }
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_language_selected(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let setting = language_setting(index);
            st.config.language = Some(setting.to_string()).filter(|s| s != "auto");
            if let Err(e) = config::save(&st.config_path, &st.config) {
                st.view.message = Some(Msg::new("Saving failed: {0}", [e.to_string()]));
            }
            let code = i18n::resolve(st.config.language.as_deref(), &env_var);
            apply_language(&ui, &mut st, code);
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_theme_selected(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let id = theme::id_at(index);
            // the default is stored as "not set": old and new configuration files stay interchangeable
            st.config.theme = Some(id.to_string()).filter(|t| t != theme::DEFAULT);
            if let Err(e) = config::save(&st.config_path, &st.config) {
                st.view.message = Some(Msg::new("Saving failed: {0}", [e.to_string()]));
            }
            render(&ui, &st);
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_recheck(move || {
            if let Some(ui) = weak.upgrade() {
                prepare_in_background(&ui, &state, false);
            }
        });
    }
    for install in [true, false] {
        let (weak, state) = (ui.as_weak(), state.clone());
        let handler = move || {
            if let Some(ui) = weak.upgrade() {
                profile_action(&ui, &state, install);
            }
        };
        if install {
            ui.on_profile_install(handler)
        } else {
            ui.on_profile_remove(handler)
        }
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_open_logs(move || {
            if let Some(ui) = weak.upgrade() {
                open_logs(&ui, &state);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_make_bundle(move || {
            if let Some(ui) = weak.upgrade() {
                make_bundle(&ui, &state);
            }
        });
    }
    for restore in [false, true] {
        let (weak, state) = (ui.as_weak(), state.clone());
        let handler = move || {
            if let Some(ui) = weak.upgrade() {
                cache_action(&ui, &state, restore);
            }
        };
        if restore {
            ui.on_cache_restore(handler)
        } else {
            ui.on_cache_rebuild(handler)
        }
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_rebuild_prefix(move || {
            if let Some(ui) = weak.upgrade() {
                prepare_in_background(&ui, &state, true);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_action(move || {
            if let Some(ui) = weak.upgrade() {
                install_action(&ui, &state);
            }
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_base_edited(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            if let Err(m) = read_install_inputs(&ui, &mut st) {
                st.view.message = Some(m);
            } else {
                st.view.message = None;
                if let Ok(l) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
                    st.install.base = Some(l.base);
                }
                let _ = save_install_state(&st);
            }
            refresh_install(&ui, &state, &mut st);
            render(&ui, &st);
        });
    }
    {
        let state = state.clone();
        ui.on_install_cancel(move || state.lock().unwrap().install_cancel.store(true, std::sync::atomic::Ordering::SeqCst));
    }
    {
        // "Browse ...": the desktop's own file dialog through the portal (the only way to a host file inside a Flatpak sandbox)
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_browse(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_busy(true);
            let (weak2, state2) = (weak.clone(), state.clone());
            std::thread::spawn(move || {
                let picked = portal::pick("The Secret World installer", false);
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    match picked {
                        Ok(Some(path)) => ui.set_install_import_path(path.display().to_string().into()),
                        Ok(None) => {}
                        Err(e) => st.view.message = Some(Msg::new("The file dialog is not available: {0}. Type the path of the installer instead.", [e])),
                    }
                });
            });
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        // "Use an existing installation …": pick TheSecretWorldDX11.exe with the desktop's file dialog, then adopt it in place (nothing is copied or changed)
        ui.on_install_adopt_browse(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_busy(true);
            let (weak2, state2) = (weak.clone(), state.clone());
            let state_in = state2.clone();
            std::thread::spawn(move || {
                // native Windows: the folder of the installation; Linux/Flatpak: the client file
                let picked = if cfg!(windows) { portal::pick("The Secret World game folder", true) } else { portal::pick("TheSecretWorldDX11.exe", false) };
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    match picked {
                        Ok(Some(path)) => match install::adopt(&path) {
                            Ok(exe) => {
                                let prefix = model::install_layout(&ui.get_install_base(), home_dir().as_deref()).ok().map(|l| l.prefix()).filter(|p| p.join("user.reg").is_file());
                                st.install.client_exe = Some(exe.clone());
                                let _ = save_install_state(st);
                                adopt_into_config(ui, &state_in, st, install::Adoption { client_exe: exe, prefix });
                            }
                            Err(e) => install_failed(st, e),
                        },
                        Ok(None) => {}
                        Err(e) => st.view.message = Some(Msg::new("The file dialog is not available: {0}. Type the path of the installer instead.", [e])),
                    }
                });
            });
        });
    }
    {
        // Windows: "Install a new copy in another folder": the user picks an empty folder; every existing installation is ignored from then on (never changed) and
        // the original installer is later started with exactly this folder
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_new_copy(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_busy(true);
            let (weak2, state2) = (weak.clone(), state.clone());
            let state_in = state2.clone();
            std::thread::spawn(move || {
                let picked = portal::pick("New folder for The Secret World (empty)", true);
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    match picked {
                        Ok(Some(path)) => start_new_copy(ui, &state_in, st, path),
                        Ok(None) => {}
                        Err(e) => st.view.message = Some(Msg::new("The file dialog is not available: {0}.", [e])),
                    }
                });
            });
        });
    }
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_use_ignored_again(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            st.install.use_ignored_again();
            st.install.apply_search();
            let _ = save_install_state(&st);
            st.view.message = Some(Msg::plain("The ignored installation is visible to the launcher again."));
            refresh_install(&ui, &state, &mut st);
            render(&ui, &st);
        });
    }
    {
        // Windows: missing Direct3D libraries. The user decides: nothing is downloaded or started without this click.
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_directx(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let layout = match model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
                Ok(l) => l,
                Err(m) => {
                    st.view.message = Some(m);
                    return render(&ui, &st);
                }
            };
            let Some(curl) = doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()) else {
                st.view.message = Some(Msg::new("curl was not found. Download Microsoft's DirectX installer yourself from {0}", [tsw_launch::dxsetup::PAGE.to_string()]));
                return render(&ui, &st);
            };
            st.install_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            let cancel = st.install_cancel.clone();
            ui.set_busy(true);
            ui.set_install_downloading(true);
            drop(st);
            let (weak2, state2) = (weak.clone(), state.clone());
            std::thread::spawn(move || {
                let sink = install_sink(&weak2, &state2, Msg::plain("Microsoft's DirectX installer was downloaded and verified."));
                // download, hash check, signature check - the installer is started only when all of them passed
                let prepared = tsw_launch::dxsetup::fetch(&layout, &install::Downloader { curl: &curl, cancel: &cancel, sink: &sink })
                    .map_err(|e| e.to_string())
                    .and_then(|file| tsw_launch::dxsetup::verify_signature(&file).map(|()| file));
                let started = prepared.and_then(|file| tsw_launch::launch::spawn_detached(&mut tsw_launch::dxsetup::start_command(&file), None).map(|_| ()).map_err(|e| e.to_string()));
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    ui.set_install_downloading(false);
                    st.view.progress = None;
                    st.view.message = Some(match started {
                        Ok(()) => Msg::plain("Microsoft's DirectX installer was started. Follow its steps (Windows may ask for permission), then click “Check again”."),
                        Err(e) if e == install::InstallError::Cancelled.to_string() => Msg::plain("Cancelled. The partial download is kept and will be resumed."),
                        Err(e) => {
                            Msg::new("Microsoft's DirectX installer could not be prepared: {0}. It was not started. You can download it yourself from {1}", [e, tsw_launch::dxsetup::PAGE.to_string()])
                        }
                    });
                    render(ui, st);
                });
            });
        });
    }
    {
        // Windows: look again after the DirectX installer ran
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_recheck_directx(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            st.view.directx_missing = tsw_launch::winnative::directx_missing_now().into_iter().map(String::from).collect();
            st.view.message =
                Some(if st.view.directx_missing.is_empty() { Msg::plain("All required DirectX libraries are present.") } else { Msg::new("Still missing: {0}", [st.view.directx_missing.join(", ")]) });
            render(&ui, &st);
        });
    }
    {
        // Windows: the user picked one of several installations that were found; the choice is stored like any adopted installation
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_use_detected(move |index| {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let Some(path) = usize::try_from(index).ok().and_then(|i| st.view.detected.get(i).cloned()) else { return };
            match install::adopt(&path) {
                Ok(exe) => {
                    st.install.client_exe = Some(exe.clone());
                    let _ = save_install_state(&st);
                    adopt_into_config(&ui, &state, &mut st, install::Adoption { client_exe: exe, prefix: None });
                }
                Err(e) => install_failed(&mut st, e),
            }
            render(&ui, &st);
        });
    }
    {
        // "Start the original patcher": it fetches and completes the game data and continues after an interruption
        let (weak, state) = (ui.as_weak(), state.clone());
        ui.on_install_patcher(move || {
            let Some(ui) = weak.upgrade() else { return };
            let mut st = state.lock().unwrap();
            let layout = model::install_layout(&ui.get_install_base(), home_dir().as_deref());
            let Some(rt) = wine_program() else {
                st.view.message = Some(messages::problem(&discover::Problem::NoRuntime));
                return render(&ui, &st);
            };
            let started = layout.and_then(|l| {
                let patcher = install::patcher_to_run(&install::assess(&l, &st.install, None).stage).ok_or_else(|| Msg::plain("There is no original patcher to start yet."))?;
                tsw_launch::launch::spawn_detached(&mut install::patcher_command(&l, &rt.wine, &patcher), None).map(|_| ()).map_err(|e| Msg::new("Installation step failed: {0}", [e.to_string()]))
            });
            match started {
                Ok(()) => st.view.message = Some(Msg::plain("The original patcher was started. It downloads the game data and continues after an interruption.")),
                Err(m) => st.view.message = Some(m),
            }
            render(&ui, &st);
        });
    }
    {
        let (weak, state, may_close) = (ui.as_weak(), state.clone(), may_close.clone());
        ui.on_play(move || {
            let Some(ui) = weak.upgrade() else { return };
            if ui.get_install_mode() {
                if !ui.get_setup_consents_ready() {
                    return;
                }
                // the big button installs: one press starts the whole standard installation; every finished step continues with the next
                {
                    let mut st = state.lock().unwrap();
                    st.install_auto = true;
                    st.view.message = None;
                }
                let mut guard = state.lock().unwrap();
                if let Err(m) = read_install_inputs(&ui, &mut guard) {
                    guard.view.message = Some(m);
                    return render(&ui, &guard);
                }
                if let Ok(l) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
                    guard.install.base = Some(l.base);
                }
                let _ = save_install_state(&guard);
                drop(guard);
                return install_action(&ui, &state);
            }
            if let Err(e) = save(&ui, &state) {
                let mut st = state.lock().unwrap();
                st.view.message = Some(e);
                render(&ui, &st);
                return;
            }
            let path = state.lock().unwrap().config_path.clone();
            ui.set_running(true);
            {
                let mut st = state.lock().unwrap();
                set_status(&mut st, Msg::plain("The game is starting …"), 0);
                render(&ui, &st);
            }
            let (weak2, state2, may_close2) = (ui.as_weak(), state.clone(), may_close.clone());
            // The CLI checks, starts the client detached and exits at once; wait for that off the UI thread.
            std::thread::spawn(move || {
                let result = model::start_game_child(&model::cli_binary(), &path);
                match result {
                    // Inside a Flatpak the sandbox ends with this process: stay until the game has ended, then close.
                    Ok(Some(mut child)) => {
                        // started: minimize (a real minimized window, reachable from the panel), then watch the game
                        post(&weak2, &state2, |ui, st| {
                            set_status(st, Msg::plain("The game is running. This window closes when you quit the game."), 0);
                            ui.window().set_minimized(true);
                        });
                        let _ = child.wait();
                        post(&weak2, &state2, move |ui, _| leave(ui, &may_close2));
                    }
                    other => post(&weak2, &state2, move |ui, st| match other {
                        // Kicked off: the client runs on its own, nothing of ours has to stay. Close like the original launcher,
                        // without the exit question (nothing was asked for) and without touching the game.
                        Ok(_) => leave(ui, &may_close2),
                        Err(msg) => {
                            ui.set_running(false);
                            st.view.message = Some(msg.clone());
                            set_status(st, msg, 2);
                        }
                    }),
                }
            });
        });
    }
}

// ---- INSTALLATION card -----------------------------------------------------------------------------------------------

fn home_dir() -> Option<PathBuf> {
    // HOME on Linux; USERPROFILE on Windows
    std::env::var_os("HOME").filter(|v| !v.is_empty()).or_else(|| std::env::var_os("USERPROFILE").filter(|v| !v.is_empty())).map(PathBuf::from)
}

/// Copy what the window shows into the persisted decisions (consent is for exactly the official address), without saving.
fn read_install_inputs(ui: &MainWindow, st: &mut State) -> Result<(), Msg> {
    st.install.download_consent = ui.get_install_consent().then(|| install::OFFICIAL.url.to_string());
    // the confirmation is bound to the fingerprint of the file in the cache, never a general permission
    let sha = cached_sha(st);
    st.install.confirmed_sha256 = match (ui.get_install_unverified(), sha) {
        (true, Some(h)) => Some(h),
        (true, None) => return Err(Msg::plain("The installer fingerprint is still being computed; confirm again in a moment")),
        (false, _) => None,
    };
    st.install.pinned_sha256 = None;
    Ok(())
}

/// The installation state file: next to the launcher configuration (the same file as `install::state_path()` in the program; derived from the window's own
/// configuration path so that a test with a private configuration never touches the real settings).
fn install_state_file(config_path: &Path) -> Option<PathBuf> {
    config_path.parent().filter(|d| !d.as_os_str().is_empty()).map(|d| d.join("install-state"))
}

fn save_install_state(st: &State) -> Result<(), Msg> {
    match install_state_file(&st.config_path) {
        Some(p) => st.install.save(&p).map_err(|e| Msg::new("Installation step failed: {0}", [e])),
        None => Ok(()),
    }
}

/// The cached SHA-256 of the installer, only if it still belongs to the file on disk (size and modification time).
fn cached_sha(st: &State) -> Option<String> {
    let layout = model::install_layout(&st.install.base.as_ref()?.display().to_string(), home_dir().as_deref()).ok()?;
    let m = std::fs::metadata(layout.installer()).ok()?;
    st.installer_hash.as_ref().filter(|h| h.size == m.len() && h.modified == m.modified().ok()).map(|h| h.sha256.clone())
}

/// Hash the cached installer in a background thread when the cache is stale (never on the UI thread); the result is handed over
/// through `post` and redraws the card.
fn kick_hash(ui: &MainWindow, state: &Shared, st: &mut State) {
    let Ok(layout) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) else {
        return;
    };
    let path = layout.installer();
    let Ok(meta) = std::fs::metadata(&path) else {
        st.installer_hash = None;
        return;
    };
    if st.hash_pending || cached_sha(st).is_some() {
        return;
    }
    st.hash_pending = true;
    let (weak, state2) = (ui.as_weak(), state.clone());
    std::thread::spawn(move || {
        let result = tsw_launch::sha256::file_sha256(&path).ok();
        let (size, modified) = (meta.len(), meta.modified().ok());
        let state3 = state2.clone();
        post(&weak, &state2, move |ui, st| {
            st.hash_pending = false;
            st.installer_hash = result.map(|(sha256, _)| InstallerHash { size, modified, sha256 });
            refresh_install(ui, &state3, st);
        });
    });
}

/// Recompute the INSTALLATION card from the inputs and the file system (read-only; starts the background hash when needed).
fn refresh_install(ui: &MainWindow, state: &Shared, st: &mut State) {
    // (the user's "install a new copy" decision was handed to the detection at start-up and by every change of it; see `State::apply_search`)
    st.view.ignored = st.install.ignored.clone();
    let import_given = !ui.get_install_import_path().trim().is_empty();
    kick_hash(ui, state, st);
    let sha = cached_sha(st);
    ui.set_install_unverified(st.install.confirmed_sha256.is_some() && st.install.confirmed_sha256 == sha);
    match model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
        Ok(layout) => {
            let (status, label, enabled) = model::install_status(&layout, &st.install, import_given, sha.as_deref());
            st.view.install = Some((status, label));
            ui.set_install_action_enabled(enabled);
            let next = install::assess(&layout, &st.install, sha.as_deref());
            // Files of the game existing is not enough: Funcom's own patcher has to confirm that nothing is left to fetch (see `install::gate`).
            let gate = current_gate(st, next.next);
            st.view.needs_install = next.next != install::Next::Play || gate != install::Gate::Open;
            st.view.install_waiting = gate == install::Gate::Working;
            st.view.first_run = gate == install::Gate::Open && matches!(next.stage, install::Stage::Empty | install::Stage::FoldersReady | install::Stage::PartialDownload { .. });
            st.view.install_hint = match gate {
                install::Gate::Working => Some(Msg::plain(
                    "The original Funcom program is working (download or check). When it shows “Play”, close its window: this window then checks the result by itself and continues.",
                )),
                install::Gate::Unverified => Some(Msg::plain(
                    "The game data is not confirmed as complete yet. The original patcher checks it and repairs what is missing: start it with the button. Once it confirms that nothing is left to fetch, this window continues by itself and closes the patcher.",
                )),
                install::Gate::Open => None,
            };
            if gate != install::Gate::Open {
                let hint = st.view.install_hint.clone().unwrap_or_else(|| Msg::plain(""));
                st.view.install = Some((hint, Msg::plain("Start the original patcher")));
                ui.set_install_action_enabled(gate == install::Gate::Unverified);
            }
            st.view.install_label = Some(match (gate, next.stage.clone()) {
                (install::Gate::Working, _) => Msg::plain("FUNCOM IS WORKING …"),
                (install::Gate::Unverified, _) => Msg::plain("VERIFY INSTALLATION"),
                (_, install::Stage::Empty | install::Stage::FoldersReady) => Msg::plain("INSTALL THE GAME"),
                _ => Msg::plain("CONTINUE INSTALLATION"),
            });
            st.view.location = compact_path(&layout.base);
            if cfg!(windows) {
                windows_install_view(st, &layout, next.stage.clone(), gate);
                st.view.directx_missing = tsw_launch::winnative::directx_missing_now().into_iter().map(String::from).collect();
            }
            st.view.install_notice = st
                .install
                .base
                .as_ref()
                .filter(|b| install::is_legacy_base(b))
                .map(|_| Msg::plain("An earlier installation inside the Flatpak data folder was found. It is only reported and never used or changed; the game is installed in the folder above."));
        }
        Err(m) => {
            st.view.install = Some((m, Msg::plain("Create folders")));
            ui.set_install_action_enabled(false);
        }
    }
}

/// Every installation folder the launcher can see right now (the client's parent folders): detected ones plus the ones named in the configuration.
fn existing_installation_dirs(st: &State) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = discover::find_native_installs(&tsw_launch::winnative::search_roots()).into_iter().filter_map(|c| c.parent().map(Path::to_path_buf)).collect();
    for c in [st.config.client_exe.as_ref(), st.install.client_exe.as_ref()].into_iter().flatten() {
        if let Some(p) = c.parent() {
            if !v.iter().any(|x| tsw_launch::winnative::overlaps(x, p)) {
                v.push(p.to_path_buf());
            }
        }
    }
    v
}

/// The user chose `path` for a NEW copy: check it, then ignore every existing installation (nothing there is touched) and remember the folder.
fn start_new_copy(ui: &MainWindow, state: &Shared, st: &mut State, path: PathBuf) {
    let mut ignored = st.install.ignored.clone();
    for d in existing_installation_dirs(st) {
        if !ignored.iter().any(|x| tsw_launch::winnative::overlaps(x, &d)) {
            ignored.push(d);
        }
    }
    let target = match install::validate_new_target(&path, &ignored) {
        Ok(t) => t,
        Err(why) => {
            st.view.message = Some(Msg::new("This folder cannot be used for a new copy: {0}", [why]));
            return render(ui, st);
        }
    };
    let overlaps_ignored = |c: &PathBuf| ignored.iter().any(|d| tsw_launch::winnative::overlaps(c, d));
    // a stored choice that points into an ignored installation is dropped (the folder itself stays in the ignored list and can be used again)
    let mut cfg = st.config.clone();
    if cfg.client_exe.as_ref().is_some_and(overlaps_ignored) {
        cfg.client_exe = None;
    }
    if st.install.client_exe.as_ref().is_some_and(overlaps_ignored) {
        st.install.client_exe = None;
    }
    st.install.ignored = ignored;
    st.install.target = Some(target.clone());
    st.install.apply_search();
    if let Err(e) = config::save(&st.config_path, &cfg) {
        st.view.message = Some(Msg::new("The configuration could not be saved: {0}", [e.to_string()]));
        return render(ui, st);
    }
    st.config = cfg;
    ui.set_client_override("".into());
    st.install_auto = false;
    let _ = save_install_state(st);
    st.view.message = Some(Msg::new(
        "The existing installation is now ignored by the launcher (nothing there was changed). The new copy goes into {0}. Choose how to install it below.",
        [target.display().to_string()],
    ));
    refresh_install(ui, state, st);
    render(ui, st);
    // re-detect after this handler returned (it holds the state lock that the background work needs)
    let (weak, state) = (ui.as_weak(), state.clone());
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = weak.upgrade() {
            prepare_in_background(&ui, &state, false);
        }
    });
}

/// Windows main view: show the folder that matters (the game, not the launcher's data folder) and say plainly which of the three situations applies:
/// not installed, installation found but not confirmed complete, or (not shown here, the window then offers "Play") ready.
fn windows_install_view(st: &mut State, layout: &install::Layout, stage: install::Stage, gate: install::Gate) {
    let found = install::find_installation(layout, &st.install);
    let game_dir = found.as_ref().and_then(|f| f.client.parent().map(Path::to_path_buf)).or_else(|| tsw_launch::winnative::default_game_dir(&tsw_launch::winnative::real_env));
    // a chosen new folder is shown while nothing is installed there yet
    let game_dir = if found.is_none() { st.install.target.clone().or(game_dir) } else { game_dir };
    st.view.location = game_dir.map(|d| d.display().to_string()).unwrap_or_default();
    st.view.install_state = match (&stage, gate) {
        (_, install::Gate::Working) => None, // the hint under the folder already explains what Funcom's program is doing
        (install::Stage::PatcherReady { .. }, _) => Some((Msg::plain("Installation found, but incomplete: the original patcher still has to fetch the game data."), 1)),
        (install::Stage::Installed { .. }, install::Gate::Unverified) => Some((Msg::plain("Installation found, but not confirmed complete. The original patcher has to check it."), 1)),
        (install::Stage::Installed { .. }, _) => None,
        _ => Some((Msg::plain("Not installed."), 0)),
    };
    // all installations on this computer, offered while none has been chosen (a stored choice is the user's decision)
    st.view.detected = if st.config.client_exe.as_ref().is_some_and(|p| p.is_file()) { Vec::new() } else { discover::find_native_installs(&tsw_launch::winnative::search_roots()) };
}

/// Is the installation ready, may it be started, or does it wait for Funcom's own programs? Only while files of the game exist; an installation the launcher
/// configuration already names is the user's decision and always open.
fn current_gate(st: &State, next: install::Next) -> install::Gate {
    // the same decision the command line makes (`install::gate_for`); only the window can observe a run and so knows `run_ok`
    install::gate_for(next, config_names_client(st), &st.evidence, st.run_ok)
}

/// Follow one run of the original patcher: a new run starts when a patcher process appears, and when it is gone the run is judged
/// (`install::RunWatch::finished_cleanly`): closed after it announced that it is done, or interrupted.
fn track_run(st: &mut State, now: std::time::Instant) {
    if st.evidence.patcher_running {
        if !st.run_active {
            (st.run, st.run_active, st.run_ok) = (install::RunWatch::default(), true, false);
        }
        if let Some(sample) = st.evidence.sample {
            st.run.observe(now, sample);
        }
    } else if st.run_active {
        st.run_active = false;
        st.run_ok = st.run.finished_cleanly();
    }
}

/// Every second while the main button is in installation mode: collect finished Funcom programs, then look - off the UI thread - at their processes and at
/// the patcher's verdict, and show the result. When Funcom confirmed completion, the installation is taken over by itself (no second click, no restart).
fn watch_tick(ui: &MainWindow, state: &Shared) {
    let (layout, install_state) = {
        let mut st = state.lock().unwrap();
        st.funcom_children.retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        if st.patcher_child.as_mut().is_some_and(|c| !matches!(c.try_wait(), Ok(None))) {
            st.patcher_child = None;
            st.close_attempted = false;
        }
        // installation mode, or a patcher of ours that may still have to be closed after Funcom's confirmation
        if !(st.view.needs_install || st.patcher_child.is_some()) || st.watch_busy {
            return;
        }
        let Ok(layout) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) else { return };
        st.watch_busy = true;
        (layout, st.install.clone())
    };
    let (weak, state2) = (ui.as_weak(), state.clone());
    std::thread::spawn(move || {
        let evidence = install::gather_evidence(&layout, &install_state);
        let state3 = state2.clone();
        post(&weak, &state2, move |ui, st| {
            st.watch_busy = false;
            st.evidence = evidence;
            track_run(st, std::time::Instant::now());
            refresh_install(ui, &state3, st);
            adopt_when_ready(ui, &state3, st);
            close_patcher_when_confirmed(ui, &state3, st);
        });
    });
}

/// Funcom's own comparison of this run said that nothing is left to fetch (`install::should_close` lists every condition): ask the original patcher to
/// close, the way a click on its close button does (Wine's `taskkill` WITHOUT `/F`), once per run, in a background thread. If it does not close, it stays
/// open and the user closes it.
fn close_patcher_when_confirmed(ui: &MainWindow, state: &Shared, st: &mut State) {
    if !install::should_close(&st.evidence, st.patcher_child.is_some(), st.close_attempted) {
        return;
    }
    let Ok(layout) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) else { return };
    let Some(rt) = wine_program() else { return };
    st.close_attempted = true;
    let (weak, state2) = (ui.as_weak(), state.clone());
    std::thread::spawn(move || {
        let outcome = install::close_patcher(&layout, &rt.wine, std::time::Duration::from_secs(15));
        post(&weak, &state2, move |_, st| {
            st.view.message = Some(match outcome {
                install::Closed::Closed => Msg::plain("The original patcher confirmed that nothing is left to fetch and was closed."),
                install::Closed::StillOpen => Msg::plain("The original patcher confirmed that nothing is left to fetch but did not close. You can close its window yourself."),
                install::Closed::Failed(e) => Msg::new("The original patcher could not be asked to close ({0}). You can close its window yourself.", [e]),
            });
        });
    });
}

/// The big button is back to PLAY (Funcom confirmed, or the files were there all along) but the configuration does not name the client yet: store it,
/// exactly what the click on the old "CONTINUE INSTALLATION" did.
fn adopt_when_ready(ui: &MainWindow, state: &Shared, st: &mut State) {
    if st.view.needs_install || config_names_client(st) {
        return;
    }
    let Ok(layout) = model::install_layout(&ui.get_install_base(), home_dir().as_deref()) else { return };
    if let Some(a) = install::adoption_for(&layout, &st.install) {
        adopt_into_config(ui, state, st, a);
    }
}

/// `/home/me/Games/The Secret World` -> `~/Games/The Secret World`.
fn compact_path(p: &Path) -> String {
    match home_dir() {
        Some(h) => match p.strip_prefix(&h) {
            Ok(rest) => format!("~/{}", rest.display()),
            Err(_) => p.display().to_string(),
        },
        None => p.display().to_string(),
    }
}

/// After a finished installation step: go on with the next one by itself while the user's last action was the big button and the step succeeded.
/// It stops where the user is needed (the original installer's own dialogs) and at every failure.
fn continue_install(ui: &MainWindow, state: &Shared, st: &mut State) {
    let failed = matches!(st.view.status, Some((_, 2)));
    if !st.install_auto || failed {
        st.install_auto = false;
        return;
    }
    let weak = ui.as_weak();
    let state = state.clone();
    let _ = weak.upgrade_in_event_loop(move |ui| install_action(&ui, &state));
}

fn install_failed(st: &mut State, e: impl std::fmt::Display) {
    st.install_auto = false;
    let msg = Msg::new("Installation step failed: {0}", [e.to_string()]);
    st.view.message = Some(msg.clone());
    set_status(st, msg, 2);
}

/// Progress sink that shows measured progress in the window and reports the end of the step.
fn install_sink(weak: &slint::Weak<MainWindow>, state: &Shared, done: Msg) -> impl Fn(Update) + Send + Sync + 'static {
    let (weak, state) = (weak.clone(), state.clone());
    move |u: Update| {
        let (weak2, state2, done) = (weak.clone(), state.clone(), done.clone());
        post(&weak2, &state2, move |_, st| match u {
            Update::Progress(p) => st.view.progress = Some(p),
            Update::Done(Outcome::Finished(_)) => {
                st.view.progress = None;
                set_status(st, done, 0);
            }
            Update::Done(Outcome::Failed(detail)) => {
                st.view.progress = None;
                if detail != install::InstallError::Cancelled.to_string() {
                    install_failed(st, detail);
                } else {
                    set_status(st, Msg::plain("Cancelled. The partial download is kept and will be resumed."), 1);
                }
            }
        });
    }
}

/// No Wine/Proton runner is installed: with the user's consent download the one pinned GE-Proton into the launcher's own data folder
/// (checksum-verified, see `tsw_launch::runtimedl`); without consent only say what is missing.
/// What an installation step needs from the Wine runtime: its program. Native Windows needs none: the commands start the original programs themselves and ignore
/// the (empty) path.
struct WineProgram {
    wine: PathBuf,
}

fn wine_program() -> Option<WineProgram> {
    if !tsw_launch::winnative::Capabilities::current().wine_runtime {
        return Some(WineProgram { wine: PathBuf::new() });
    }
    discover::find_runtimes(&Env::from_process()).into_iter().next().map(|r| WineProgram { wine: r.wine })
}

fn provide_runner(ui: &MainWindow, state: &Shared, mut st: std::sync::MutexGuard<'_, State>) {
    if !ui.get_install_consent() {
        st.view.message = Some(Msg::plain("No Wine/Proton runner was found. Agree to the downloads and press the button again."));
        return render(ui, &st);
    }
    let path = std::env::var_os("PATH");
    let (Some(curl), Some(tar)) = (doctor::find_program(Path::new("curl"), path.as_deref()), doctor::find_program(Path::new("tar"), path.as_deref())) else {
        install_failed(&mut st, "curl or tar was not found");
        return render(ui, &st);
    };
    let layout = match model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
        Ok(l) => l,
        Err(m) => {
            st.view.message = Some(m);
            return render(ui, &st);
        }
    };
    let runtime_dir = layout.runtime();
    st.install_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
    let cancel = st.install_cancel.clone();
    ui.set_busy(true);
    ui.set_install_downloading(true);
    drop(st);
    let (weak, state2) = (ui.as_weak(), state.clone());
    std::thread::spawn(move || {
        let sink = install_sink(&weak, &state2, Msg::plain("The runner was downloaded, verified and unpacked."));
        let _ = tsw_launch::runtimedl::install(&runtime_dir, &tsw_launch::runtimedl::GE_PROTON, true, &tar, &install::Downloader { curl: &curl, cancel: &cancel, sink: &sink });
        let state3 = state2.clone();
        post(&weak, &state2, move |ui, st| {
            ui.set_busy(false);
            ui.set_install_downloading(false);
            refresh_install(ui, &state3, st);
            continue_install(ui, &state3, st);
        });
    });
}

/// Install the native Direct3D profile into the managed prefix (components fetched from Microsoft, verified; no-op when one is installed).
/// Returns a short English reason if it could not be done. Runs on a worker thread (download).
fn install_shader_profile(layout: &install::Layout, consent: bool, approval: &dyn Fn(&str) -> bool, sink: &dyn Sink, cancel: &AtomicBool) -> Result<(), String> {
    let prefix = layout.prefix();
    if prefix::installed_native(&prefix).is_some() {
        return Ok(());
    }
    let path = std::env::var_os("PATH");
    let find = |n: &str| doctor::find_program(Path::new(n), path.as_deref()).ok_or(format!("{n} was not found"));
    let (curl, bsdtar) = (find("curl")?, find("bsdtar")?);
    let data = Env::from_process().data_dir().ok_or("no data folder")?;
    let profile = &prefix::PROFILE_D3D_NATIVE_MS2010;
    let dir = tsw_launch::dxredist::provide(&data, profile, consent, approval, &bsdtar, &install::Downloader { curl: &curl, cancel, sink }).map_err(|e| e.to_string())?;
    prefix::install_native(profile, &dir, &prefix)
}

fn install_action(ui: &MainWindow, state: &Shared) {
    if !ui.get_setup_consents_ready() {
        return;
    }
    let weak = ui.as_weak();
    let mut st = state.lock().unwrap();
    let layout = match model::install_layout(&ui.get_install_base(), home_dir().as_deref()) {
        Ok(l) => l,
        Err(m) => {
            st.view.message = Some(m);
            return render(ui, &st);
        }
    };
    if let Err(m) = read_install_inputs(ui, &mut st) {
        st.view.message = Some(m);
        return render(ui, &st);
    }
    st.install.base = Some(layout.base.clone());
    if let Err(m) = save_install_state(&st) {
        st.view.message = Some(m);
        return render(ui, &st);
    }
    let import = ui.get_install_import_path().trim().to_string();
    let sha = cached_sha(&st);
    let next = install::assess(&layout, &st.install, sha.as_deref()).next;
    // files exist but Funcom has not confirmed completion: a running program is never interrupted; otherwise the original patcher checks (and repairs)
    let next = match (next, current_gate(&st, next)) {
        (install::Next::Play, install::Gate::Working) => {
            st.view.message =
                Some(Msg::plain("The original Funcom program is working (download or check). When it shows “Play”, close its window: this window then checks the result by itself and continues."));
            return render(ui, &st);
        }
        (install::Next::Play, install::Gate::Unverified) => install::Next::RunPatcher,
        (n, _) => n,
    };
    match next {
        install::Next::PrepareFolders => match install::prepare(&layout) {
            Ok(()) => {
                set_status(&mut st, Msg::plain("The folders were created."), 0);
                let (weak3, state3) = (ui.as_weak(), state.clone());
                let auto = st.install_auto;
                if auto {
                    let _ = weak3.upgrade_in_event_loop(move |ui| install_action(&ui, &state3));
                }
            }
            Err(e) => install_failed(&mut st, e),
        },
        install::Next::GetInstaller if !import.is_empty() => match install::import_installer(&layout, Path::new(&import), st.install.pinned_sha256.as_deref()) {
            Ok((_, hash)) => {
                ui.set_install_import_path(SharedString::new());
                st.view.message = Some(Msg::new("Installer copied. Its SHA-256 is {0}. It is not trusted until you confirm exactly this fingerprint.", [hash]));
            }
            Err(e) => install_failed(&mut st, e),
        },
        install::Next::GetInstaller => {
            let Some(curl) = doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()) else {
                install_failed(&mut st, "curl was not found");
                return render(ui, &st);
            };
            st.install_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            let (cancel, inst) = (st.install_cancel.clone(), st.install.clone());
            ui.set_busy(true);
            ui.set_install_downloading(true);
            drop(st);
            let (weak2, state2) = (weak.clone(), state.clone());
            std::thread::spawn(move || {
                let sink = install_sink(&weak2, &state2, Msg::plain("The installer was downloaded and verified."));
                let _ = install::download(&layout, &install::OFFICIAL, &inst, &install::Downloader { curl: &curl, cancel: &cancel, sink: &sink });
                let state3 = state2.clone();
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    ui.set_install_downloading(false);
                    refresh_install(ui, &state3, st);
                    continue_install(ui, &state3, st);
                });
            });
            return;
        }
        next @ (install::Next::CreatePrefix | install::Next::RunInstaller)
            if tsw_launch::winnative::Capabilities::current().wine_runtime
                && (next == install::Next::CreatePrefix || (ui.get_install_consent() && prefix::installed_native(&layout.prefix()).is_none())) =>
        {
            let Some(rt) = wine_program() else {
                return provide_runner(ui, state, st);
            };
            ui.set_busy(true);
            let shaders = ui.get_install_consent();
            let microsoft_accepted = ui.get_microsoft_consent();
            st.install_cancel.store(false, std::sync::atomic::Ordering::SeqCst);
            let cancel = st.install_cancel.clone();
            drop(st);
            let (weak2, state2) = (weak.clone(), state.clone());
            std::thread::spawn(move || {
                let sink = install_sink(&weak2, &state2, Msg::plain("The Wine prefix is ready."));
                // the prefix first, then (with consent) the native shader profile into THIS prefix; a failure there is reported, the prefix stays
                let created = next != install::Next::CreatePrefix || install::create_prefix(&layout, &rt.wine, &sink).is_ok();
                let mut shaders_ok = true;
                if created && shaders {
                    let sink2 = install_sink(&weak2, &state2, Msg::plain("The Wine prefix and the native shader profile are ready."));
                    if let Err(e) = install_shader_profile(&layout, true, &|_| microsoft_accepted, &sink2, &cancel) {
                        shaders_ok = false;
                        sink2.update(Update::Done(Outcome::Failed(e)));
                    }
                }
                let state3 = state2.clone();
                post(&weak2, &state2, move |ui, st| {
                    ui.set_busy(false);
                    if !created || !shaders_ok {
                        st.install_auto = false;
                    }
                    refresh_install(ui, &state3, st);
                    continue_install(ui, &state3, st);
                });
            });
            return;
        }
        install::Next::CreatePrefix => unreachable!("handled by the prefix preparation guard"),
        install::Next::RunInstaller => {
            let Some(rt) = wine_program() else {
                return provide_runner(ui, state, st);
            };
            let Some(sha) = sha else {
                st.view.message = Some(Msg::plain("Checking the installer fingerprint …"));
                return render(ui, &st);
            };
            match install::run_gate(&layout, &st.install, &install::OFFICIAL, &sha) {
                Err(e) => install_failed(&mut st, e),
                Ok(()) => match install::installer_command_to(&layout, &rt.wine, st.install.target.as_deref())
                    .ok_or_else(|| "the new folder cannot be handed to the installer safely; choose another folder".to_string())
                    .and_then(|mut cmd| tsw_launch::launch::spawn_detached_child(&mut cmd, None).map_err(|e| e.to_string()))
                {
                    Ok(child) => {
                        // from now on the registry is read again, but only for entries that are new compared with this moment
                        st.install.note_installer_started(tsw_launch::winnative::registered_game_dirs());
                        st.install.apply_search();
                        let _ = save_install_state(&st);
                        st.funcom_children.push(child);
                        st.evidence.installer_running = true;
                        st.install_auto = false;
                        st.view.message = Some(Msg::new(
                            "{0} {1}",
                            [
                                if tsw_launch::winnative::Capabilities::current().wine_runtime {
                                    Msg::plain("Please use the suggested default settings while installing The Secret World and do not change the installation path in particular. The suggested Windows path is inside the Wine/Proton environment managed by our launcher.")
                                } else {
                                    model::windows_installer_notice(st.install.target.as_deref())
                                },
                                Msg::plain("When the installer is finished, press the button again."),
                            ],
                        ))
                    }
                    Err(e) => install_failed(&mut st, e),
                },
            }
        }
        install::Next::RunPatcher => {
            let Some(rt) = wine_program() else {
                return provide_runner(ui, state, st);
            };
            match install::patcher_to_run(&install::assess(&layout, &st.install, sha.as_deref()).stage) {
                Some(patcher) => match tsw_launch::launch::spawn_detached_child(&mut install::patcher_command(&layout, &rt.wine, &patcher), None) {
                    Ok(child) => {
                        st.patcher_child = Some(child);
                        st.close_attempted = false;
                        st.evidence.patcher_running = true;
                        st.evidence.verdict = tsw_launch::patchlog::Verdict::Unknown;
                        st.install_auto = false;
                        st.view.message = Some(Msg::plain("The original patcher was started. This window notices by itself when it reports that nothing is left to fetch."))
                    }
                    Err(e) => install_failed(&mut st, e),
                },
                None => install_failed(&mut st, "the patcher was not found"),
            }
        }
        install::Next::Play => match install::adoption_for(&layout, &st.install) {
            Some(a) => adopt_into_config(ui, state, &mut st, a),
            None => install_failed(&mut st, "the client was not found"),
        },
    }
    refresh_install(ui, state, &mut st);
    render(ui, &st);
}

/// Store an installation in the configuration (a working entry is never replaced) and re-detect.
fn adopt_into_config(ui: &MainWindow, state: &Shared, st: &mut State, a: install::Adoption) {
    let mut cfg = st.config.clone();
    if !tsw_launch::winnative::stored_paths_honored() {
        // paused detection: the path in an old configuration was not chosen by the user and must not protect itself against the real choice
        cfg.client_exe = None;
    }
    match install::merge_config(&mut cfg, &a).map_err(|e| e.to_string()).and_then(|()| config::save(&st.config_path, &cfg).map_err(|e| e.to_string())) {
        Ok(()) => {
            st.config = cfg;
            // a deliberate choice (by hand, or the installation the launcher itself started and Funcom confirmed): stored paths count from now on
            st.install.choice_made = true;
            st.install.apply_search();
            let _ = save_install_state(st);
            st.view.message = Some(Msg::new("Installation stored: {0}", [a.client_exe.display().to_string()]));
            ui.set_client_override(a.client_exe.display().to_string().into());
            if let Some(p) = &a.prefix {
                ui.set_prefix_override(p.display().to_string().into());
            }
            // re-detect after this handler returned (it holds the state lock that the background work needs)
            let (weak, state) = (ui.as_weak(), state.clone());
            let _ = slint::invoke_from_event_loop(move || {
                if let Some(ui) = weak.upgrade() {
                    prepare_in_background(&ui, &state, false);
                }
            });
        }
        Err(e) => install_failed(st, e),
    }
}

fn main() -> Result<(), slint::PlatformError> {
    if matches!(std::env::args().nth(1).as_deref(), Some("--version" | "-V")) {
        println!("{}", tsw_launch::project::version_line("tsw-launcher"));
        return Ok(());
    }
    let config_path = config::write_path().unwrap_or_else(|| PathBuf::from("tsw-launch.config"));
    let (config, load_note) = match config::load(&config_path) {
        Ok(c) => (c, None),
        Err(_) if !config_path.exists() => (Config::default(), None),
        Err(e) => (Config::default(), Some(Msg::new("Configuration not readable: {0}", [e.to_string()]))),
    };
    let ui = MainWindow::new()?;
    // native Windows has no Wine, Proton, DXVK or Microsoft-component options: the window hides them
    ui.set_windows(!tsw_launch::winnative::Capabilities::current().wine_runtime);
    // The desktop identity: Wayland app id and X11 WM_CLASS. KDE, GNOME and the task bar find the icon through the desktop entry of
    // the same name (packaging/linux). Needs the Slint context (created with the first window) and must happen before show();
    // it must match that file (test below).
    slint::set_xdg_app_id(APP_ID)?;
    let language = startup_language(&config, &env_var);
    i18n::select_language(language);
    let s = Settings::from_config(&config);
    ui.set_skip_intro(s.skip_intro);
    ui.set_fps_limit(s.fps as i32);
    ui.set_fps_unlimited(s.fps_unlimited);
    ui.set_fps_hud(s.fps_hud);
    ui.set_gamescope_enabled(s.gamescope);
    ui.set_native_dir(s.native_dir.clone().into());
    ui.set_diag_enabled(s.diag_on);
    ui.set_diag_level_index(i32::from(s.diag_detailed));
    ui.set_client_override(s.client_override.into());
    ui.set_runner_override(s.runner_override.into());
    ui.set_prefix_override(s.prefix_override.into());
    ui.set_virtual_desktop(s.virtual_desktop);
    ui.set_desktop_size(s.desktop_size.into());
    if let Some(img) = model::background_image(&config_path).and_then(|p| slint::Image::load_from_path(&p).ok()) {
        ui.set_background_image(img);
        ui.set_has_background(true);
    }
    let mut view = View { message: load_note, ..View::default() };
    view.status = Some((Msg::plain("Getting ready …"), 0));
    let state: Shared = Arc::new(Mutex::new(State {
        config_path,
        config,
        catalog: Catalog::new(language),
        view,
        install: Default::default(),
        install_cancel: Default::default(),
        install_auto: false,
        installer_hash: None,
        hash_pending: false,
        evidence: Default::default(),
        watch_busy: false,
        funcom_children: Vec::new(),
        patcher_child: None,
        close_attempted: false,
        run: Default::default(),
        run_active: false,
        run_ok: false,
    }));
    render(&ui, &state.lock().unwrap());

    {
        let mut st = state.lock().unwrap();
        st.install = install_state_file(&st.config_path).and_then(|p| install::State::load(&p).ok()).unwrap_or_default();
        st.install.apply_search();
        // an earlier layout (inside the Flatpak data folder) is only reported; the window works with the default folder
        ui.set_install_base(st.install.base.as_ref().filter(|b| !install::is_legacy_base(b)).map(|b| b.display().to_string()).unwrap_or_default().into());
        ui.set_install_consent(st.install.download_consent.as_deref() == Some(install::OFFICIAL.url));
        ui.set_microsoft_consent(Env::from_process().data_dir().is_some_and(|p| tsw_launch::dxredist::terms_accepted(&p)));
        refresh_install(&ui, &state, &mut st);
        render(&ui, &st);
    }
    // Launcher update hint (Flatpak only, silent when offline or not published): the HOST's Flatpak does the actual, signed update.
    if Path::new("/.flatpak-info").is_file() {
        let (weak, state2) = (ui.as_weak(), state.clone());
        std::thread::spawn(move || {
            let curl = doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from("curl"));
            // the channel is the Flatpak branch this launcher was installed from; a local build (any other branch) is not compared with the repository
            let info = std::fs::read_to_string("/.flatpak-info").unwrap_or_default();
            let channel = tsw_launch::selfupdate::branch(&info).unwrap_or_else(|| "stable".into());
            let status = if tsw_launch::selfupdate::is_channel(&channel) {
                tsw_launch::selfupdate::check(Path::new("/.flatpak-info"), &curl, tsw_launch::project::FLATPAK_REPO_URL, &tsw_launch::project::flatpak_ref_for(&channel))
            } else {
                tsw_launch::selfupdate::Status::Unknown("not an update channel".into())
            };
            if matches!(status, tsw_launch::selfupdate::Status::Available { .. }) {
                post(&weak, &state2, |_, st| {
                    st.view.message = Some(Msg::new(
                        "A launcher update is available. Install it with your system\'s software center, or run: flatpak update {0}. Then restart the launcher.",
                        [tsw_launch::project::APP_ID.to_string()],
                    ))
                });
            }
        });
    }
    let may_close = Arc::new(AtomicBool::new(false));
    ui.set_game_ends_with_launcher(tsw_launch::gamescope::in_flatpak(Path::new("/.flatpak-info"), std::env::var_os("FLATPAK_ID").as_deref()));
    wire(&ui, &state, &may_close);
    ui.show()?;
    fit_window(&ui);
    set_resolution_modes(&ui, &state.lock().unwrap(), &[]);
    refresh_resolution_modes(&ui, &state);
    prepare_in_background(&ui, &state, false);
    // the watcher of the installation (cheap while nothing is installing: it returns at once outside installation mode)
    let timer = slint::Timer::default();
    {
        let (weak, state) = (ui.as_weak(), state.clone());
        timer.start(slint::TimerMode::Repeated, std::time::Duration::from_secs(1), move || {
            if let Some(ui) = weak.upgrade() {
                watch_tick(&ui, &state);
            }
        });
    }
    let result = slint::run_event_loop();
    drop(timer);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_app_id_matches_the_desktop_entry_and_the_icon_files_of_the_package() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("packaging/linux");
        let desktop = std::fs::read_to_string(dir.join(format!("{APP_ID}.desktop"))).expect("desktop entry named after the app id");
        let value = |k: &str| desktop.lines().find_map(|l| l.strip_prefix(&format!("{k}="))).map(str::to_string);
        assert_eq!(value("Icon").as_deref(), Some(APP_ID), "Icon= is the icon theme name = app id");
        assert_eq!(value("StartupWMClass").as_deref(), Some(APP_ID), "X11 windows are matched through StartupWMClass");
        assert_eq!(value("Type").as_deref(), Some("Application"));
        assert_eq!(value("Exec").as_deref(), Some("tsw-launcher"), "installed packages find the binary on PATH");
        assert!(value("Name").is_some() && value("Categories").is_some_and(|c| c.contains("Game")));
        assert!(!desktop.contains("Terminal=true"));
        for size in [16, 24, 32, 48, 64, 128, 256, 512] {
            let f = dir.join(format!("icons/hicolor/{size}x{size}/apps/{APP_ID}.png"));
            assert!(f.is_file(), "missing icon {}", f.display());
        }
        assert!(dir.join(format!("icons/hicolor/scalable/apps/{APP_ID}.svg")).is_file());
        // reverse-DNS shape required by Flatpak and the desktop-entry spec for D-Bus activatable names
        assert!(APP_ID.split('.').count() >= 3 && APP_ID.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric())));
    }

    #[test]
    fn the_selector_index_and_the_stored_setting_map_both_ways() {
        assert_eq!((language_index(None), language_index(Some("auto"))), (0, 0));
        assert_eq!((language_index(Some("en")), language_index(Some("de"))), (1, 2));
        assert_eq!(language_index(Some("klingon")), 0, "an unknown stored value shows as automatic");
        assert_eq!((language_setting(0), language_setting(1), language_setting(2)), ("auto", "en", "de"));
        assert_eq!((language_setting(-1), language_setting(99)), ("auto", "auto"));
        for (i, (code, _, _)) in i18n::LANGUAGES.iter().enumerate() {
            assert_eq!(language_setting(language_index(Some(code))), *code);
            assert_eq!(language_index(Some(code)), i as i32 + 1);
        }
    }

    #[test]
    fn the_start_language_follows_the_saved_choice_then_the_locale() {
        let de = |k: &str| (k == "LANG").then(|| "de_DE.UTF-8".to_string());
        let none = |_: &str| None;
        let cfg = |l: Option<&str>| Config { language: l.map(String::from), ..Config::default() };
        assert_eq!(startup_language(&cfg(None), &de), "de");
        assert_eq!(startup_language(&cfg(Some("auto")), &de), "de");
        assert_eq!(startup_language(&cfg(Some("en")), &de), "en", "the saved choice wins over the locale");
        assert_eq!(startup_language(&cfg(Some("de")), &none), "de");
        // without any locale variable: English on Linux; on Windows the display language of the system (English as the fallback for unsupported languages)
        #[cfg(not(windows))]
        assert_eq!(startup_language(&cfg(None), &none), "en");
        #[cfg(windows)]
        assert!(["en", "de"].contains(&startup_language(&cfg(None), &none)));
    }

    #[test]
    fn the_language_choice_survives_a_restart_through_the_config_file() {
        let d = std::env::temp_dir().join(format!("tsw-gui-lang-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let path = d.join("config");
        let mut c = Config { skip_intro: Some(true), env: vec![("FOO".into(), "bar".into())], ..Config::default() };
        let s = Settings { language: "de".into(), ..Settings::from_config(&c) };
        s.apply(&mut c).unwrap();
        config::save(&path, &c).unwrap();
        let back = config::load(&path).unwrap();
        assert_eq!(back.language.as_deref(), Some("de"));
        assert_eq!(startup_language(&back, &|_| None), "de");
        assert_eq!(back.env, vec![("FOO".to_string(), "bar".to_string())], "other keys survive");
        // choosing "automatic" again removes the key
        let s = Settings { language: "auto".into(), ..Settings::from_config(&back) };
        let mut c2 = back.clone();
        s.apply(&mut c2).unwrap();
        config::save(&path, &c2).unwrap();
        assert_eq!(config::load(&path).unwrap().language, None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn progress_details_use_the_languages_number_format_and_only_measured_values() {
        let p = Progress { phase: Phase::Installing, step: "x".into(), done: Some(41_200_000), total: Some(65_000_000), rate: Some(12_000_000.0), eta_secs: Some(3) };
        assert_eq!(progress_detail(&p, &Catalog::new("de")), "63 % · 41,2 MB von 65,0 MB · 12,0 MB/s · noch 3 s");
        assert_eq!(progress_detail(&p, &Catalog::new("en")), "63% · 41.2 MB of 65.0 MB · 12.0 MB/s · 3 s left");
        assert_eq!(progress_detail(&Progress::indeterminate(Phase::Checking, "x"), &Catalog::new("de")), "", "nothing invented");
    }
}
