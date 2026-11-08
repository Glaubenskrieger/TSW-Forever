use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use tsw_launch::config::{self, Config};
use tsw_launch::diag;
use tsw_launch::diaglog::{self, DiagLevel, DiagLog};
use tsw_launch::discover::{self, Env};
use tsw_launch::doctor::{self, Finding, Level};
use tsw_launch::gamescope;
use tsw_launch::launch;
use tsw_launch::prefix;
use tsw_launch::prefs;
use tsw_launch::runner::Runner;
use tsw_launch::shadercache;
use tsw_launch::{client_arguments, credentials_for_exe, redacted, ArgOptions, LaunchError};

const USAGE: &str = "\
tsw-launch (proof of concept)

usage: tsw-launch [--config <file>] [--exe <TheSecretWorld[DX11].exe>] [options]
       tsw-launch cache check|quarantine|restore [--exe <client>]   inspect / rebuild (reversibly) the client's local shader cache
       tsw-launch patch status|check|apply|verify   signed client patches (off until the launcher contains a trusted key)
       tsw-launch install status|prepare|download|import|runner|prefix|run|patcher|adopt|finish   set up or adopt an installation (see below)
       tsw-launch prefix init|check --prefix <dir> --runner <path-to-wine>   create/verify the Wine prefix (see below)

  --config <file>      key = value file (default: $TSW_LAUNCH_CONFIG or ~/.config/tsw-launch/config); options below override it
  --exe <path>         client executable (never modified)
  --dx9 | --dx11       render path flag passed to the client (default: --dx11)
  --no-console         pass -noconsole
  --virtual-desktop WxH  run the client inside Wine's virtual desktop of that size (compatibility mode, wine runners only;
                       works around a message loop with real full-screen X11 windows, see documentation/architecture.md)
  --resolution WxH     set the client's DisplayResolution preference (default with --virtual-desktop: its size)
  --skip-intro | --play-intro   pass the client's own -novideo switch (skips the start-up videos) or not (overrides the config)
  --detach             start the client and exit at once (exit code 0 = started); the client keeps running on its own,
                       without a terminal and with its output discarded (add --log-file to keep it)
  --stay-alive         with --detach: do not exit until the client has ended (the window front end waits for it; always the case inside a Flatpak)
  --log-file <file>    with --detach: write the client's output to this file (it may contain account names later)
  --keep-driver-warning  do not pre-set the client's \"do not notify again\" preference for the outdated-graphics-driver dialog
                       (default: the preference is set in the per-user Prefs_3.xml; no game file is touched)
  --runner <r>         native | wine | <path-to-wine-binary>   (default: native on Windows, wine elsewhere)
  --prefix <dir>       WINEPREFIX (wine runners)
  --diagnostics <off|basic|detailed>   optional private diagnostic log (config key diagnostics); `diag dir` prints its folder, `diag bundle` writes a reduced text for bug reports
  --fps-limit <30..240|unlimited>   frame-rate limit for the game (default 60; config key fps_limit); --fps-hud | --no-fps-hud: DXVK FPS counter
  --display-mode <fullscreen|windowed|borderless>   opt-in: set the client's DisplayMode preference at every start (config key display_mode; default: leave the client's own choice)
  --wm-decorations <auto|on|off>   window-manager decorations of the game window (config key wm_decorations; default auto: removed for a full-screen Wine game, which is what makes Alt+Tab return to full screen)
  --gamescope | --no-gamescope   run the client inside a plain gamescope window, internal = output resolution, no scaling (default off; config key gamescope).
                       Optional: the normal start needs no gamescope. Not combinable with --virtual-desktop
  --gamescope-output WxH   resolution of that gamescope window (default: the monitor's native size; config key gamescope_output)
  --native-dir <dir> | --native-profile <id>   for `prefix native-install` (default profile d3d-native-v7, UNCONFIRMED hypothesis)
  --wrapper <program>  run the client under a wrapper program (for example gamemoderun, or gdb for diagnosis); repeatable via --wrapper-arg
  --wrapper-arg <arg>  argument for the wrapper, in order (the client command line follows them)
  --env NAME=VALUE     extra environment variable for the client process (repeatable; not for secrets)
  --arg <value>        extra client argument, passed through verbatim (repeatable)
  --arg-env FLAG=NAME  pass FLAG followed by the value of the environment variable NAME (for -loginkey/-statindex):
                       keeps the value out of shell history; it is masked in all output and never stored
  --doctor             check prerequisites (client, wine, prefix, 32-bit fonts, display, Vulkan) and exit.
                       exit code 0 = fine, 4 = warnings, 3 = a start is not possible
  --print-args         print the client arguments (secrets masked) and exit without starting anything
  --dry-run            like --print-args, but also print program and working directory
  --now <unix>         override the clock (testing only)

cache check | quarantine | restore : inspect the client's local shader cache; quarantine = rename (never delete) so the game rebuilds it; restore undoes it
prefix native-install : install the profile's native DLLs from --native-dir after verifying size+MD5 (backed up); native-remove undoes it
install : guided installation below ONE folder (--base): game/, prefix/, downloads/. status shows the next step; prepare creates the folders (refuses
          non-empty game/prefix); download needs --consent-download (https from the official source, SHA-256 pinned; the pin is our own measurement,
          not a vendor value) - or use import --file <installer you checked yourself>; runner downloads the pinned GE-Proton (--consent-download);
          shaders installs the native Direct3D profile from Microsoft (--consent-download); prefix creates the Wine/Proton prefix; run starts the
          original installer (an unknown file needs --confirm-sha256 of exactly that file); adopt --path <exe|folder> uses an existing installation
          without copying or changing it; finish stores the result in the configuration (a working entry is never replaced).
prefix init : create the prefix with `wineboot -u` if needed, install the DXVK/vkd3d components the client uses from the
              runtime, verify the window metrics, write a manifest (tsw-launch-prefix.txt). prefix check : verify only.

A start refuses (exit 3) when the client or the wine program is missing; other findings are printed as warnings.
";

#[derive(Default)]
struct Cli {
    config: Option<PathBuf>,
    exe: Option<PathBuf>,
    dx11: Option<bool>,
    no_console: bool,
    keep_driver_warning: bool,
    detach: bool,
    /// With `--detach`: stay alive until the client has ended (a front end that waits for the game; always so inside a Flatpak sandbox).
    stay_alive: bool,
    log_file: Option<PathBuf>,
    skip_intro: Option<bool>,
    virtual_desktop: Option<(u32, u32)>,
    resolution: Option<(u32, u32)>,
    runner: Option<String>,
    prefix: Option<PathBuf>,
    native_dir: Option<PathBuf>,
    native_profile: Option<String>,
    fps_limit: Option<tsw_launch::fps::FpsLimit>,
    fps_hud: Option<bool>,
    display_mode: Option<prefs::DisplayMode>,
    wm_decorations: Option<tsw_launch::windowing::WmDecorations>,
    gamescope: Option<bool>,
    gamescope_output: Option<(u32, u32)>,
    diag: Option<DiagLevel>,
    supervised: bool,
    env: Vec<(String, String)>,
    wrapper: Vec<String>,
    extra: Vec<String>,
    arg_env: Vec<(String, String)>,
    dry: bool,
    print_args: bool,
    doctor: bool,
    now: Option<u64>,
}

fn split_pair(v: &str, what: &str) -> Result<(String, String), LaunchError> {
    v.split_once('=').filter(|(a, b)| !a.is_empty() && !b.is_empty()).map(|(a, b)| (a.to_string(), b.to_string())).ok_or_else(|| LaunchError::Usage(format!("{what} needs the form A=B")))
}

fn parse(args: Vec<String>) -> Result<Cli, LaunchError> {
    let mut c = Cli::default();
    let mut it = args.into_iter();
    let need = |it: &mut std::vec::IntoIter<String>, f: &str| it.next().ok_or_else(|| LaunchError::Usage(format!("{f} needs a value")));
    while let Some(a) = it.next() {
        match a.as_str() {
            "--config" => c.config = Some(PathBuf::from(need(&mut it, "--config")?)),
            "--exe" => c.exe = Some(PathBuf::from(need(&mut it, "--exe")?)),
            "--dx9" => c.dx11 = Some(false),
            "--dx11" => c.dx11 = Some(true),
            "--no-console" => c.no_console = true,
            "--virtual-desktop" => {
                c.virtual_desktop = Some(config::parse_size(&need(&mut it, "--virtual-desktop")?).ok_or_else(|| LaunchError::Usage("--virtual-desktop needs a size like 1920x1200".into()))?)
            }
            "--resolution" => c.resolution = Some(config::parse_size(&need(&mut it, "--resolution")?).ok_or_else(|| LaunchError::Usage("--resolution needs a size like 1920x1200".into()))?),
            "--detach" => c.detach = true,
            "--stay-alive" => c.stay_alive = true,
            "--log-file" => c.log_file = Some(PathBuf::from(need(&mut it, "--log-file")?)),
            "--skip-intro" => c.skip_intro = Some(true),
            "--play-intro" => c.skip_intro = Some(false),
            "--keep-driver-warning" => c.keep_driver_warning = true,
            "--runner" => c.runner = Some(need(&mut it, "--runner")?),
            "--prefix" => c.prefix = Some(PathBuf::from(need(&mut it, "--prefix")?)),
            "--fps-limit" => c.fps_limit = Some(tsw_launch::fps::parse(&need(&mut it, "--fps-limit")?).map_err(LaunchError::Usage)?),
            "--display-mode" => c.display_mode = Some(prefs::DisplayMode::parse(&need(&mut it, "--display-mode")?).map_err(LaunchError::Usage)?),
            "--wm-decorations" => c.wm_decorations = Some(tsw_launch::windowing::WmDecorations::parse(&need(&mut it, "--wm-decorations")?).map_err(LaunchError::Usage)?),
            "--gamescope" => c.gamescope = Some(true),
            "--no-gamescope" => c.gamescope = Some(false),
            "--gamescope-output" => {
                let v = need(&mut it, "--gamescope-output")?;
                c.gamescope_output = Some(tsw_launch::config::parse_size(&v).ok_or_else(|| LaunchError::Usage(format!("--gamescope-output {v:?} must look like 1920x1200")))?)
            }
            "--diagnostics" => c.diag = Some(DiagLevel::parse(&need(&mut it, "--diagnostics")?).map_err(LaunchError::Usage)?),
            "--supervised" => c.supervised = true,
            "--fps-hud" => c.fps_hud = Some(true),
            "--no-fps-hud" => c.fps_hud = Some(false),
            "--native-dir" => c.native_dir = Some(PathBuf::from(need(&mut it, "--native-dir")?)),
            "--native-profile" => c.native_profile = Some(need(&mut it, "--native-profile")?),
            "--wrapper" => c.wrapper.insert(0, need(&mut it, "--wrapper")?),
            "--wrapper-arg" => c.wrapper.push(need(&mut it, "--wrapper-arg")?),
            "--env" => c.env.push(split_pair(&need(&mut it, "--env")?, "--env")?),
            "--arg" => c.extra.push(need(&mut it, "--arg")?),
            "--arg-env" => c.arg_env.push(split_pair(&need(&mut it, "--arg-env")?, "--arg-env")?),
            "--print-args" => c.print_args = true,
            "--dry-run" => c.dry = true,
            "--doctor" => c.doctor = true,
            "--now" => c.now = Some(need(&mut it, "--now")?.parse().map_err(|_| LaunchError::Usage("--now needs an integer".into()))?),
            "-h" | "--help" => return Err(LaunchError::Usage(USAGE.to_string())),
            other => return Err(LaunchError::Usage(format!("unknown option {other:?}\n\n{USAGE}"))),
        }
    }
    Ok(c)
}

/// Effective settings: command line over configuration file over platform defaults.
struct Settings {
    exe: Option<PathBuf>,
    runner: Runner,
    prefix: Option<PathBuf>,
    opt: ArgOptions,
    env: Vec<(String, String)>,
    suppress_driver_warning: bool,
    virtual_desktop: Option<(u32, u32)>,
    resolution: Option<(u32, u32)>,
    /// Frame-rate limit (default 60, free 30..240, or unlimited) and the DXVK FPS counter, for the game process only.
    fps: tsw_launch::fps::FpsLimit,
    fps_hud: bool,
    /// Resolved Gamescope plan (`None` = off, the normal start path) and the gamescope program found for it.
    display_mode: Option<prefs::DisplayMode>,
    wm_decorations: tsw_launch::windowing::WmDecorations,
    gamescope_plan: Option<gamescope::Plan>,
    gamescope: Option<PathBuf>,
    diag: DiagLevel,
    /// Plain-language remarks from the detection (for example: several runtimes found).
    notes: Vec<String>,
}

fn resolve(cli: &Cli) -> Result<Settings, LaunchError> {
    let cfg_path = cli.config.clone().or_else(config::default_path);
    let cfg = match &cfg_path {
        Some(p) => config::load(p).map_err(|e| LaunchError::Usage(e.to_string()))?,
        None => Config::default(),
    };
    // Explicit values (command line > configuration file) win; everything else comes from the shared detection
    // (the same code the GUI uses): cached result, else a fresh search. Nothing is hard-coded here.
    let explicit = Config {
        client_exe: cli.exe.clone().or(cfg.client_exe.clone()),
        runner: cli.runner.clone().or(cfg.runner.clone()).or_else(|| cfg!(windows).then(|| "native".to_string())),
        prefix: cli.prefix.clone().or(cfg.prefix.clone()),
        ..Config::default()
    };
    let found = discover::resolve(&Env::from_process(), &explicit, discover::cache_path().as_deref()).map_err(|p| LaunchError::Usage(p.message()))?;
    let prefix = Some(found.prefix.clone());
    let mut extra = cli.extra.clone();
    for (flag, var) in &cli.arg_env {
        let value = std::env::var(var).map_err(|_| LaunchError::Usage(format!("--arg-env {flag}={var}: environment variable {var} is not set")))?;
        extra.push(flag.clone());
        extra.push(value);
    }
    if cli.skip_intro.or(cfg.skip_intro).unwrap_or(false) {
        extra.push("-novideo".to_string());
    }
    let mut env = cfg.env.clone();
    env.extend(cli.env.iter().cloned());
    let legacy_display = cfg.game_resolution.is_none() && !cfg.game_managed;
    let plan = gamescope::plan(
        cli.gamescope.or(if legacy_display { cfg.gamescope } else { None }).unwrap_or(false),
        cli.gamescope_output.or(cfg.gamescope_output),
        tsw_launch::display::system_primary_mode(),
    )
    .map_err(LaunchError::Usage)?;
    let wine_runner = matches!(Runner::from_spec(Some(found.runner.as_str()), None), Runner::Wine { .. });
    // Wine: the chosen size becomes a private Wine desktop. Native Windows has no such desktop: the same choice is written to the client's own preferences.
    let game_desktop = if wine_runner {
        tsw_launch::display::game_desktop(cfg.game_resolution, &if cfg.game_resolution.is_some() { tsw_launch::display::available_modes() } else { Vec::new() }, true, plan.is_some())
            .map_err(LaunchError::Usage)?
    } else {
        None
    };
    let native_resolution = if wine_runner { None } else { cfg.game_resolution };
    let legacy_desktop = if legacy_display { cfg.virtual_desktop } else { None };
    let legacy_resolution = if legacy_display { cfg.resolution } else { None };
    let virtual_desktop = game_desktop.or(cli.virtual_desktop).or(legacy_desktop);
    if plan.is_some() {
        if virtual_desktop.is_some() {
            return Err(LaunchError::Usage("Gamescope cannot be combined with the virtual desktop: turn one of them off".into()));
        }
        if !matches!(Runner::from_spec(Some(found.runner.as_str()), None), Runner::Wine { .. }) {
            return Err(LaunchError::Usage("Gamescope needs a Wine/Proton runner (gamescope is a Linux program)".into()));
        }
    }
    let flatpak = gamescope::in_flatpak(Path::new("/.flatpak-info"), std::env::var_os("FLATPAK_ID").as_deref());
    let gamescope = plan.as_ref().and_then(|_| gamescope::find_gamescope(std::env::var_os("PATH").as_deref(), flatpak, Path::new(gamescope::FLATPAK_EXTENSION_BIN)));
    Ok(Settings {
        display_mode: cli.display_mode.or(cfg.display_mode),
        wm_decorations: cli.wm_decorations.or(cfg.wm_decorations).unwrap_or_default(),
        gamescope_plan: plan.clone(),
        gamescope,
        exe: Some(found.client_exe.clone()),
        runner: Runner::from_spec(Some(found.runner.as_str()), prefix.clone()),
        notes: found.notes.clone(),
        prefix,
        opt: ArgOptions { dx11: cli.dx11.or(cfg.dx11).unwrap_or(true), no_console: cli.no_console || cfg.no_console.unwrap_or(false), extra },
        env,
        virtual_desktop,
        fps: cli.fps_limit.or(cfg.fps_limit).unwrap_or_default(),
        fps_hud: cli.fps_hud.or(cfg.fps_hud).unwrap_or(false),
        diag: cli.diag.or(cfg.diagnostics).unwrap_or_default(),
        // Explicit private game desktop > legacy explicit options; no automatic monitor-size override.
        resolution: tsw_launch::display::launch_resolution(
            game_desktop.or(native_resolution),
            plan.as_ref().map(|p| p.size),
            cli.resolution.or(legacy_resolution),
            cli.virtual_desktop.or(legacy_desktop),
        ),
        suppress_driver_warning: !cli.keep_driver_warning && cfg.suppress_driver_warning.unwrap_or(true),
    })
}

fn print_findings(findings: &[Finding], only_problems: bool) {
    for f in findings {
        if only_problems && f.level == Level::Ok {
            continue;
        }
        let tag = match f.level {
            Level::Ok => "ok   ",
            Level::Warn => "WARN ",
            Level::Fail => "FAIL ",
        };
        eprintln!("[{tag}] {}", f.what);
        if !f.hint.is_empty() {
            eprintln!("        -> {}", f.hint);
        }
    }
}

fn shadercache_state() -> Option<PathBuf> {
    shadercache::default_state_file()
}

/// `tsw-launch cache check|quarantine|restore`: user-controlled handling of the client's local shader cache (see shadercache.rs).
fn run_cache(args: Vec<String>) -> Result<ExitCode, LaunchError> {
    let mut args = args.into_iter();
    let sub = args.next().ok_or_else(|| LaunchError::Usage("cache needs check, quarantine or restore".into()))?;
    let cli = parse(args.collect())?;
    let s = resolve(&cli)?;
    let dir = s.exe.as_deref().and_then(Path::parent).map(Path::to_path_buf).ok_or_else(|| LaunchError::Usage("cache needs --exe (or client_exe in the config)".into()))?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    let running = || {
        std::fs::read_dir("/proc")
            .into_iter()
            .flatten()
            .flatten()
            .any(|e| std::fs::read_to_string(e.path().join("comm")).map(|c| c.trim_start().to_ascii_lowercase().starts_with("thesecretworld")).unwrap_or(false))
    };
    match sub.as_str() {
        "check" => {
            let findings = shadercache::check(&dir, s.prefix.as_deref(), shadercache_state().as_deref());
            print_findings(&findings, false);
            let q = shadercache::quarantined(&dir);
            if !q.is_empty() {
                println!("{} quarantined cop{} next to the cache (restore with `tsw-launch cache restore`)", q.len(), if q.len() == 1 { "y" } else { "ies" });
            }
            Ok(ExitCode::from(doctor::exit_code(&findings)))
        }
        "quarantine" | "restore" => {
            if running() {
                eprintln!("tsw-launch: the game seems to be running; close it first (it writes the cache on exit)");
                return Ok(ExitCode::from(3));
            }
            let r = if sub == "quarantine" { shadercache::quarantine(&dir, now) } else { shadercache::restore(&dir, now) };
            match r {
                Ok(p) => {
                    println!("{}: {}", if sub == "quarantine" { "shader cache set aside as" } else { "shader cache restored as" }, p.display());
                    println!(
                        "{}",
                        if sub == "quarantine" { "the game rebuilds it at the next start; undo with `tsw-launch cache restore`" } else { "a cache written since then was set aside, not overwritten" }
                    );
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("tsw-launch: cache {sub} refused: {e}");
                    Ok(ExitCode::from(3))
                }
            }
        }
        other => Err(LaunchError::Usage(format!("unknown cache command {other:?} (check, quarantine or restore)"))),
    }
}

fn run_checks(s: &Settings) -> Vec<Finding> {
    let exe = s.exe.clone().unwrap_or_default();
    let has_display = std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some();
    let mut f = doctor::check_all(&exe, &s.runner, s.prefix.as_deref(), &doctor::default_dirs(), std::env::var_os("PATH").as_deref(), has_display);
    if matches!(s.runner, Runner::Native) {
        // native Windows: no Wine prefix, Gamescope or DXVK shader cache; the Direct3D libraries are the only extra check
        f.extend(doctor::check_directx(&tsw_launch::winnative::directx_missing_now()));
        return f;
    }
    if let (Runner::Wine { wine, .. }, Some(prefix)) = (&s.runner, s.prefix.as_deref()) {
        if prefix.join("user.reg").is_file() {
            let wine = doctor::find_program(wine, std::env::var_os("PATH").as_deref()).unwrap_or_else(|| wine.clone());
            f.extend(prefix::check(prefix, prefix::runtime_files_dir(&wine).as_deref()));
        }
    }
    let flatpak = gamescope::in_flatpak(Path::new("/.flatpak-info"), std::env::var_os("FLATPAK_ID").as_deref());
    f.extend(gamescope::check(s.gamescope_plan.as_ref(), s.gamescope.as_deref(), gamescope::already_in_gamescope(|k| std::env::var(k).ok()), flatpak));
    if let Some(dir) = s.exe.as_deref().and_then(Path::parent) {
        f.extend(shadercache::check(dir, s.prefix.as_deref(), shadercache_state().as_deref()));
    }
    f
}

/// Where the client keeps this user's preferences for the chosen runner (None: not determinable).
fn prefs_dir(s: &Settings) -> Option<PathBuf> {
    match &s.runner {
        Runner::Native => prefs::native_prefs_dir(),
        Runner::Wine { .. } => {
            let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
            let prefix = s.prefix.clone().or_else(|| env("WINEPREFIX")).or_else(|| env("HOME").map(|h| h.join(".wine")))?;
            Some(prefs::wine_prefs_dir(&prefix))
        }
    }
}

/// Pre-set the client's own "do not notify again" preference for the outdated-graphics-driver dialog.
/// Dry runs only report what would happen. A failure here is a warning, never a reason to refuse the start.
fn apply_prefs(s: &Settings, write: bool) {
    if !s.suppress_driver_warning && s.resolution.is_none() && s.display_mode.is_none() {
        return;
    }
    let Some(dir) = prefs_dir(s) else {
        eprintln!("tsw-launch: WARN cannot determine the preferences directory; the driver-update dialog may appear");
        return;
    };
    if let Some((w, h)) = s.resolution {
        let v = prefs::ipoint(w, h);
        let attr = match prefs::attr_plain(&v) {
            Ok(a) => a,
            Err(e) => {
                eprintln!("tsw-launch: WARN {e}");
                return;
            }
        };
        match if write { prefs::ensure_value(&dir, prefs::RESOLUTION_PREF, &attr) } else { prefs::plan(&dir, prefs::RESOLUTION_PREF, &attr) } {
            Ok(o) if write => {
                if o != prefs::Outcome::Unchanged {
                    eprintln!("tsw-launch: DisplayResolution set to {v} ({o:?})");
                }
            }
            Ok(o) => println!("prefs: DisplayResolution {v} would be {o:?}"),
            Err(e) => eprintln!("tsw-launch: WARN {e}; the client will use its default resolution"),
        }
    }
    if let Some(m) = s.display_mode {
        if let Ok(attr) = prefs::attr_plain(m.pref_value()) {
            match if write { prefs::ensure_value(&dir, prefs::DISPLAY_MODE_PREF, &attr) } else { prefs::plan(&dir, prefs::DISPLAY_MODE_PREF, &attr) } {
                Ok(o) if write => {
                    if o != prefs::Outcome::Unchanged {
                        eprintln!("tsw-launch: DisplayMode set to {} ({o:?})", m.text());
                    }
                }
                Ok(o) => println!("prefs: DisplayMode {} would be {o:?}", m.text()),
                Err(e) => {
                    eprintln!("tsw-launch: WARN {e}; the client keeps its saved display mode")
                }
            }
        }
    }
    if !s.suppress_driver_warning {
        return;
    }
    let attr = match prefs::attr_string(prefs::DRIVER_VALUE) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("tsw-launch: WARN {e}");
            return;
        }
    };
    let result = if write { prefs::ensure_value(&dir, prefs::DRIVER_PREF, &attr) } else { prefs::plan(&dir, prefs::DRIVER_PREF, &attr) };
    match result {
        Ok(o) if write => {
            if o != prefs::Outcome::Unchanged {
                eprintln!("tsw-launch: driver-update warning suppressed via {} ({o:?})", dir.join(prefs::FILE).display());
            }
        }
        Ok(o) => println!("prefs: {} would be {o:?}", dir.join(prefs::FILE).display()),
        Err(e) => eprintln!("tsw-launch: WARN {e}; the driver-update dialog may appear"),
    }
}

fn run_prefix(args: Vec<String>) -> Result<ExitCode, LaunchError> {
    let mut args = args.into_iter();
    let sub = args.next().ok_or_else(|| LaunchError::Usage(format!("prefix needs init or check\n\n{USAGE}")))?;
    let cli = parse(args.collect())?;
    let s = resolve(&cli)?;
    let prefix = s.prefix.clone().ok_or_else(|| LaunchError::Usage("prefix needs --prefix <dir> (or prefix = in the config)".into()))?;
    let Runner::Wine { wine, .. } = &s.runner else {
        return Err(LaunchError::Usage("prefix management is for the wine runner".into()));
    };
    let wine = doctor::find_program(wine, std::env::var_os("PATH").as_deref()).unwrap_or_else(|| wine.clone());
    match sub.as_str() {
        "init" => {
            for f in doctor::check_32bit_fonts(&doctor::default_dirs()) {
                if f.level != Level::Ok {
                    print_findings(&[f], true);
                    return Ok(ExitCode::from(3));
                }
            }
            match prefix::init(&prefix, &wine) {
                Ok(m) => println!("prefix ready: {} (manifest {})", prefix.display(), m.display()),
                Err(e) => {
                    eprintln!("tsw-launch: prefix init failed: {e}");
                    return Ok(ExitCode::from(3));
                }
            }
            let findings = prefix::check(&prefix, prefix::runtime_files_dir(&wine).as_deref());
            print_findings(&findings, true);
            Ok(ExitCode::from(doctor::exit_code(&findings)))
        }
        "check" => {
            let findings = prefix::check(&prefix, prefix::runtime_files_dir(&wine).as_deref());
            print_findings(&findings, false);
            Ok(ExitCode::from(doctor::exit_code(&findings)))
        }
        "native-install" => {
            let id = cli.native_profile.as_deref().unwrap_or(prefix::PROFILE_D3D_NATIVE_V7.id);
            let profile = prefix::native_profile(id).ok_or_else(|| LaunchError::Usage(format!("unknown --native-profile {id:?}")))?;
            let dir = cli.native_dir.as_deref().ok_or_else(|| LaunchError::Usage("native-install needs --native-dir <directory with the 32-bit DLLs>".into()))?;
            match prefix::install_native(profile, dir, &prefix) {
                Ok(()) => println!("profile {id} installed in {} ({} DLLs; undo with `prefix native-remove`)", prefix.display(), profile.files.len()),
                Err(e) => {
                    eprintln!("tsw-launch: native-install refused: {e}");
                    return Ok(ExitCode::from(3));
                }
            }
            let findings = prefix::check_manifest(&prefix);
            print_findings(&findings, false);
            Ok(ExitCode::from(doctor::exit_code(&findings)))
        }
        "native-remove" => match prefix::remove_native(&prefix) {
            Ok(id) => {
                println!("profile {id} removed from {}", prefix.display());
                Ok(ExitCode::SUCCESS)
            }
            Err(e) => {
                eprintln!("tsw-launch: native-remove refused: {e}");
                Ok(ExitCode::from(3))
            }
        },
        other => Err(LaunchError::Usage(format!("unknown prefix command {other:?} (init, check, native-install or native-remove)"))),
    }
}

/// Replace the value after `flag` (the short-lived patcher key is not a credential, but it has no place in a report either).
fn mask_flag(mut args: Vec<String>, flag: &str) -> Vec<String> {
    if let Some(i) = args.iter().position(|a| a == flag) {
        if let Some(v) = args.get_mut(i + 1) {
            *v = "<redacted>".to_string();
        }
    }
    args
}

/// Write `last-launch.txt` next to the configuration; a failure is ignored (diagnostics must never stop a start).
fn write_start_record(s: &Settings, exe: &Path, args: &[String], env: &[(String, String)]) {
    let Some(path) = config::write_path().and_then(|p| p.parent().map(|d| d.join("last-launch.txt"))) else {
        return;
    };
    let manifest = s.prefix.as_deref().and_then(|p| std::fs::read_to_string(p.join(prefix::MANIFEST)).ok()).unwrap_or_default();
    let line = |key: &str| manifest.lines().find_map(|l| l.strip_prefix(key)?.strip_prefix('=')).map(|v| v.trim().to_string());
    let record = diag::LaunchRecord {
        unix_time: SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        launcher_version: env!("CARGO_PKG_VERSION").to_string(),
        client: exe.display().to_string(),
        runner: match &s.runner {
            Runner::Wine { wine, .. } => wine.display().to_string(),
            Runner::Native => "native".to_string(),
        },
        runtime_version: line("runtime"),
        prefix: s.prefix.as_ref().map(|p| p.display().to_string()).unwrap_or_default(),
        virtual_desktop: s.virtual_desktop,
        resolution: s.resolution,
        dll_overrides: env.iter().find(|(k, _)| k == "WINEDLLOVERRIDES").map(|(_, v)| v.clone()),
        components: manifest.lines().filter_map(|l| l.strip_prefix("component=")).map(str::to_string).collect(),
        args: mask_flag(redacted(args), "-key"),
        env_names: env.iter().map(|(k, _)| k.clone()).collect(),
        fps_limit: matches!(s.runner, Runner::Wine { .. }).then(|| format!("{}{}", s.fps.text(), if s.fps_hud { " (DXVK FPS counter on)" } else { "" })),
    };
    let home = std::env::var("HOME").ok();
    let _ = diag::write_record(&path, &record.render(home.as_deref()));
}

#[cfg(unix)]
fn diaglog_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new().write(true).create(true).truncate(true).mode(0o600).open(path)
}
#[cfg(not(unix))]
fn diaglog_private_file(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::create(path)
}

/// A plain-language line for the way the game ended.
fn describe_exit(status: &std::process::ExitStatus) -> String {
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        if let Some(sig) = status.signal() {
            return format!("stopped by signal {sig} (the process was killed or crashed hard)");
        }
    }
    match status.code() {
        Some(0) => "exit code 0 (normal end)".into(),
        Some(c) => {
            format!("exit code {c} (the client or Wine reported this; the log above may tell why)")
        }
        None => "no exit code available".into(),
    }
}

/// What a diagnostic log starts with: versions, the setup, the check results. Environment variables by name only; arguments redacted.
fn log_start(log: &mut DiagLog, s: &Settings, exe: &Path, args: &[String], env: &[(String, String)], findings: &[Finding], now: u64) {
    log.event(now, &format!("tsw-launch {} · diagnostics: {}", env!("CARGO_PKG_VERSION"), s.diag.text()));
    log.event(now, &format!("client: {}", exe.display()));
    if let Runner::Wine { wine, .. } = &s.runner {
        log.event(now, &format!("runner: {}", wine.display()));
    }
    if let Some(p) = s.prefix.as_deref() {
        let manifest = std::fs::read_to_string(p.join(prefix::MANIFEST)).unwrap_or_default();
        let get = |k: &str| manifest.lines().find_map(|l| l.strip_prefix(k)?.strip_prefix('=')).unwrap_or("-").to_string();
        log.event(now, &format!("prefix: {} · runtime: {} · native profile: {} · overrides: {}", p.display(), get("runtime"), get("native_profile"), get("dll_overrides")));
        log.event(now, &format!("shader compiler setup: {}", shadercache::compiler_context(p)));
    }
    if let Some(dir) = exe.parent() {
        for f in shadercache::check(dir, s.prefix.as_deref(), shadercache_state().as_deref()) {
            log.event(now, &format!("shader cache: {}", f.what));
        }
    }
    log.event(now, &format!("frame-rate limit: {}{}", s.fps.text(), if s.fps_hud { " (DXVK FPS counter on)" } else { "" }));
    log.event(now, &format!("arguments: {}", mask_flag(redacted(args), "-key").join(" ")));
    log.event(now, &format!("extra environment (names only): {}", env.iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>().join(", ")));
    for f in findings {
        log.event(
            now,
            &format!(
                "check [{}] {}",
                match f.level {
                    Level::Ok => "ok",
                    Level::Warn => "warn",
                    Level::Fail => "FAIL",
                },
                f.what
            ),
        );
    }
}

/// Inside a Flatpak sandbox the whole sandbox ends with its main process, which would kill a "detached" client at once. So there the launcher
/// stays alive until the client has ended (the "started" line was printed before; a front end reads it and keeps waiting for the process).
fn stay_alive_in_flatpak(child: &mut std::process::Child, requested: bool, exe: Option<&Path>) {
    if requested || gamescope::in_flatpak(Path::new("/.flatpak-info"), std::env::var_os("FLATPAK_ID").as_deref()) {
        match exe.and_then(|e| e.file_name()).and_then(|n| n.to_str()) {
            // ends the tree if the client is gone but a wrapper (Gamescope) still waits for an orphaned Wine helper
            Some(name) => {
                let _ = launch::wait_for_client(child, name, std::time::Duration::from_secs(8), std::time::Duration::from_millis(500));
            }
            None => {
                let _ = child.wait();
            }
        }
    }
}

/// Start this program again, detached, to supervise the game (write the log, see the exit). The parent returns at once.
fn spawn_supervisor(original: &[String], config_file: Option<&Path>) -> Result<ExitCode, LaunchError> {
    let mut args: Vec<String> = Vec::new();
    let mut it = original.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--detach" | "--stay-alive" => {}
            "--log-file" => {
                it.next();
            }
            other => args.push(other.to_string()),
        }
    }
    args.push("--supervised".into());
    let exe = std::env::current_exe().map_err(|e| LaunchError::Io(e, "cannot find the launcher program".into()))?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(&args);
    match launch::spawn_detached_child(&mut cmd, None) {
        Ok(mut child) => {
            let dir = diaglog::log_dir(config_file.unwrap_or(Path::new("tsw-launch-config")));
            eprintln!("tsw-launch: started with diagnostic logging (supervisor pid {}); logs: {}", child.id(), dir.display());
            stay_alive_in_flatpak(&mut child, original.iter().any(|a| a == "--stay-alive"), None);
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => Err(LaunchError::Io(e, "cannot start the supervisor".into())),
    }
}

/// `tsw-launch diag bundle|dir`: where the logs are, and a reduced, re-masked text for bug reports (never sent anywhere).
fn run_diag(args: Vec<String>) -> Result<ExitCode, LaunchError> {
    let mut args = args.into_iter();
    let sub = args.next().ok_or_else(|| LaunchError::Usage("diag needs dir or bundle".into()))?;
    let cli = parse(args.collect())?;
    let config_file = cli.config.clone().or_else(config::default_path).or_else(config::write_path).unwrap_or_else(|| PathBuf::from("tsw-launch-config"));
    let dir = diaglog::log_dir(&config_file);
    match sub.as_str() {
        "dir" => {
            println!("{}", dir.display());
            Ok(ExitCode::SUCCESS)
        }
        "bundle" => {
            let now = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
            let home = std::env::var("HOME").ok();
            let saved = config::load(&config_file).ok();
            let mut parts: Vec<(&str, String)> = vec![("launcher", format!("tsw-launch {}", env!("CARGO_PKG_VERSION")))];
            if let Some(c) = &saved {
                parts.push(("configuration (environment values omitted)", config::diagnostic_text(c)));
            }
            if let Some(last) = config_file.parent().map(|d| d.join("last-launch.txt")).and_then(|p| std::fs::read_to_string(p).ok()) {
                parts.push(("last start record", last));
            }
            if let Ok(s) = resolve(&cli) {
                let findings = run_checks(&s);
                parts.push((
                    "checks",
                    findings
                        .iter()
                        .map(|f| {
                            format!(
                                "[{}] {}{}",
                                match f.level {
                                    Level::Ok => "ok",
                                    Level::Warn => "warn",
                                    Level::Fail => "FAIL",
                                },
                                f.what,
                                if f.hint.is_empty() { String::new() } else { format!("\n        -> {}", f.hint) }
                            )
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                ));
                if let Some(p) = s.prefix.as_deref() {
                    parts.push(("shader compiler setup", shadercache::compiler_context(p)));
                }
            }
            if let Some(log) = diaglog::newest(&dir).and_then(|p| std::fs::read_to_string(p).ok()) {
                parts.push(("newest launcher log", log));
            }
            let hidden: Vec<&str> = saved.as_ref().map(|c| c.env.iter().map(|(_, value)| value.as_str()).collect()).unwrap_or_default();
            diaglog::omit_values(&mut parts, &hidden);
            let (text, leaked) = diaglog::bundle(&parts, home.as_deref(), now);
            if leaked {
                eprintln!("tsw-launch: the package still contains a word that looks like a credential; it was NOT written. Please report this.");
                return Ok(ExitCode::from(3));
            }
            match diaglog::write_bundle(&dir, now, &text) {
                Ok(p) => {
                    println!("{}", p.display());
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => Err(LaunchError::Io(e, "cannot write the diagnostic package".into())),
            }
        }
        other => Err(LaunchError::Usage(format!("unknown diag command {other:?} (dir or bundle)"))),
    }
}

fn patch_auto(cli: &Cli) -> bool {
    let cfg_path = cli.config.clone().or_else(config::default_path);
    cfg_path.and_then(|p| config::load(&p).ok()).and_then(|c| c.patch_auto).unwrap_or(true)
}

/// Check for and apply client patches before the game starts; every outcome except "refused" is silent, none blocks the start.
fn patch_before_start(exe: &Path, cli: &Cli) {
    let cfg = cli.config.clone().or_else(config::default_path).and_then(|p| config::load(&p).ok()).unwrap_or_default();
    let env = Env::from_process();
    let data = env.xdg_data.clone().or_else(|| env.home.as_ref().map(|h| h.join(".local/share")));
    let (Some(root), Some(work)) = (exe.parent(), tsw_launch::patchcmd::work_dir(env.install_base.as_deref(), data.as_deref())) else {
        return;
    };
    let curl = doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from("curl"));
    let base = cfg.patch_source.clone().unwrap_or_else(|| tsw_launch::project::PATCH_BASE_URL.to_string());
    let setup = tsw_launch::patchcmd::Setup { public_key: tsw_launch::project::PATCH_PUBLIC_KEY, base_url: &base, curl: &curl, root, work: &work };
    let o = tsw_launch::patchcmd::update(&setup, &|| tsw_launch::patchcmd::game_running_in(Path::new("/proc")), &std::sync::atomic::AtomicBool::new(false));
    if !matches!(o, tsw_launch::patchcmd::Outcome::UpToDate { .. } | tsw_launch::patchcmd::Outcome::Disabled(_)) {
        eprintln!("tsw-launch: {}", tsw_launch::patchcmd::describe(&o));
    }
}

/// The window-manager decoration switch for the game process (Alt+Tab fix, see `windowing.rs`), if it applies.
fn window_decorations_env(s: &Settings, env: &[(String, String)]) -> Option<(&'static str, &'static str)> {
    let wine = matches!(s.runner, Runner::Wine { .. });
    let display_mode = s.display_mode.or_else(|| prefs_dir(s).and_then(|d| prefs::read_display_mode(&d)));
    tsw_launch::windowing::env_to_set(
        s.wm_decorations,
        tsw_launch::windowing::Context {
            wine,
            virtual_desktop: s.virtual_desktop.is_some(),
            display_mode,
            user_defined: env.iter().any(|(k, _)| k == tsw_launch::windowing::ENV) || std::env::var_os(tsw_launch::windowing::ENV).is_some(),
        },
    )
}

fn run() -> Result<ExitCode, LaunchError> {
    let mut raw: Vec<String> = std::env::args().skip(1).collect();
    if matches!(raw.first().map(String::as_str), Some("--version" | "-V")) {
        println!("{}", tsw_launch::project::version_line("tsw-launch"));
        return Ok(ExitCode::SUCCESS);
    }
    if raw.first().map(String::as_str) == Some("prefix") {
        raw.remove(0);
        return run_prefix(raw);
    }
    if raw.first().map(String::as_str) == Some("patch") {
        raw.remove(0);
        return match tsw_launch::patchcmd::run(raw) {
            Ok(code) => Ok(ExitCode::from(code)),
            Err(m) => Err(LaunchError::Usage(m)),
        };
    }
    if raw.first().map(String::as_str) == Some("install") {
        raw.remove(0);
        return match tsw_launch::installcmd::run(raw) {
            Ok(code) => Ok(ExitCode::from(code)),
            Err(m) => Err(LaunchError::Usage(m)),
        };
    }
    if raw.first().map(String::as_str) == Some("cache") {
        raw.remove(0);
        return run_cache(raw);
    }
    if raw.first().map(String::as_str) == Some("diag") {
        raw.remove(0);
        return run_diag(raw);
    }
    let original_args = raw.clone();
    let cli = parse(raw)?;
    let s = resolve(&cli)?;
    if cli.doctor {
        let findings = run_checks(&s);
        print_findings(&findings, false);
        return Ok(ExitCode::from(doctor::exit_code(&findings)));
    }
    let exe = s.exe.clone().ok_or_else(|| LaunchError::Usage(format!("no client executable: pass --exe or set client_exe in the config\n\n{USAGE}")))?;
    for n in &s.notes {
        eprintln!("tsw-launch: Hinweis: {n}");
    }
    let findings = run_checks(&s);
    print_findings(&findings, true);
    if findings.iter().any(|f| f.level == Level::Fail) {
        eprintln!("tsw-launch: a start is not possible (see FAIL above)");
        return Ok(ExitCode::from(3));
    }
    // Client patches (off until the launcher contains a trusted key): never blocks the start, never runs for dry runs.
    if !(cli.dry || cli.print_args) && !tsw_launch::project::PATCH_PUBLIC_KEY.is_empty() && patch_auto(&cli) {
        patch_before_start(&exe, &cli);
    }
    let now = cli.now.unwrap_or_else(|| SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    let cred = credentials_for_exe(&exe, now)?;
    let args = client_arguments(&cred, &s.opt);
    if cli.print_args || cli.dry {
        println!("{}", redacted(&args).join(" "));
    }
    let wine_args: Vec<String> = match s.virtual_desktop {
        Some((w, h)) if matches!(s.runner, Runner::Wine { .. }) => {
            vec!["explorer".into(), format!("/desktop=TSW,{w}x{h}")]
        }
        _ => Vec::new(),
    };
    // Gamescope (off by default): it becomes the outermost wrapper. Without it this is exactly the command of the normal path.
    let wrapper: Vec<String> = match (&s.gamescope_plan, &s.gamescope) {
        (Some(plan), Some(gs)) => plan.wrapper(gs, &cli.wrapper),
        _ => cli.wrapper.clone(),
    };
    let mut cmd = s.runner.command_full(&wrapper, &wine_args, &exe, &args);
    let mut env = s.env.clone();
    if let Some(ours) = s.prefix.as_deref().and_then(prefix::dll_overrides).filter(|_| matches!(s.runner, Runner::Wine { .. })) {
        let existing = env.iter().find(|(k, _)| k == "WINEDLLOVERRIDES").map(|(_, v)| v.clone()).or_else(|| std::env::var("WINEDLLOVERRIDES").ok());
        env.retain(|(k, _)| k != "WINEDLLOVERRIDES");
        env.push(("WINEDLLOVERRIDES".into(), prefix::merge_overrides(existing.as_deref(), &ours)));
    }
    // Diagnostic logging (off by default): with --detach a detached supervisor takes over (it writes the log and sees the exit);
    // otherwise this process does. Nothing of this exists when diagnostics are off.
    let config_file = cli.config.clone().or_else(config::default_path).or_else(config::write_path);
    let diag_on = s.diag.is_on() && !cli.dry && !cli.print_args;
    if diag_on && cli.detach && !cli.supervised {
        return spawn_supervisor(&original_args, config_file.as_deref());
    }
    let mut diag_log: Option<DiagLog> = None;
    let mut game_log_path: Option<PathBuf> = None;
    if diag_on {
        let dir = diaglog::log_dir(config_file.as_deref().unwrap_or(Path::new("tsw-launch-config")));
        match DiagLog::create(&dir, now, std::env::var("HOME").ok()) {
            Ok(l) => {
                if s.diag == DiagLevel::Detailed {
                    let stamp = diaglog::stamp_of(&l.path).unwrap_or_default();
                    let set = |env: &mut Vec<(String, String)>, k: &str, v: String| {
                        if !env.iter().any(|(n, _)| n == k) && std::env::var_os(k).is_none() {
                            env.push((k.to_string(), v));
                        }
                    };
                    if matches!(s.runner, Runner::Wine { .. }) {
                        let dxvk = dir.join(format!("dxvk-{stamp}"));
                        let _ = std::fs::create_dir_all(&dxvk);
                        set(&mut env, "WINEDEBUG", "err+all,+timestamp".into());
                        set(&mut env, "DXVK_LOG_LEVEL", "info".into());
                        set(&mut env, "DXVK_LOG_PATH", dxvk.display().to_string());
                    }
                    game_log_path = Some(dir.join(format!("game-{stamp}.log")));
                }
                diag_log = Some(l);
            }
            Err(e) => eprintln!("tsw-launch: diagnostic logging could not start ({e}); the game starts without it"),
        }
    }
    if let Some((k, v)) = window_decorations_env(&s, &env) {
        env.push((k.to_string(), v.to_string()));
    }
    if matches!(s.runner, Runner::Wine { .. }) {
        // The limit only exists in DXVK's environment of THIS process (see fps.rs); nothing is written anywhere.
        let existing = |name: &str| env.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).or_else(|| std::env::var(name).ok());
        let config = tsw_launch::fps::merge_dxvk_config(existing("DXVK_CONFIG").as_deref(), &s.fps.dxvk_options());
        let hud = s.fps_hud.then(|| tsw_launch::fps::merge_hud(existing("DXVK_HUD").as_deref()));
        env.retain(|(k, _)| k != "DXVK_CONFIG" && !(hud.is_some() && k == "DXVK_HUD"));
        env.push(("DXVK_CONFIG".into(), config));
        if let Some(h) = hud {
            env.push(("DXVK_HUD".into(), h));
        }
    }
    for (k, v) in &env {
        cmd.env(k, v);
    }
    if cli.dry || cli.print_args {
        if cli.dry {
            println!("dir: {}", cmd.get_current_dir().map(|d| d.display().to_string()).unwrap_or_default());
            println!("program: {}", cmd.get_program().to_string_lossy());
            for (k, _) in &env {
                println!("env: {k}=<set>");
            }
            if let Some(p) = &s.gamescope_plan {
                println!("gamescope: {}x{} (internal = output, no scaling)", p.size.0, p.size.1);
            }
            if matches!(s.runner, Runner::Wine { .. }) {
                println!("fps: limit {} ({}){}", s.fps.text(), s.fps.dxvk_options(), if s.fps_hud { ", DXVK FPS counter on" } else { "" });
            }
            apply_prefs(&s, false);
        }
        return Ok(ExitCode::SUCCESS);
    }
    apply_prefs(&s, true);
    write_start_record(&s, &exe, &args, &env);
    if let (Some(state), Some(dir), Some(prefix)) = (shadercache_state(), exe.parent(), s.prefix.as_deref()) {
        shadercache::record_context(&state, dir, &shadercache::compiler_context(prefix), SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0));
    }
    eprintln!("tsw-launch: starting {} ({})", exe.display(), mask_flag(redacted(&args), "-key").join(" "));
    if cli.detach {
        return match launch::spawn_detached_child(&mut cmd, cli.log_file.as_deref()) {
            Ok(mut child) => {
                eprintln!("tsw-launch: started (pid {}); the client keeps running independently", child.id());
                stay_alive_in_flatpak(&mut child, cli.stay_alive, Some(&exe));
                Ok(ExitCode::SUCCESS)
            }
            Err(e) => Err(LaunchError::Io(e, format!("cannot start {:?}", cmd.get_program()))),
        };
    }
    if let Some(log) = diag_log.as_mut() {
        log_start(log, &s, &exe, &args, &env, &findings, now);
    }
    // Waiting mode: the client's stdout/stderr (its log can contain the chat registration with nick and password) is forwarded
    // through the central masker, so a copied terminal never carries it.
    let started = std::time::Instant::now();
    let status = if diag_log.is_some() {
        // supervised: the game's output goes (masked, line by line) into the private game log of a detailed diagnosis, otherwise it is dropped
        let file = game_log_path.as_deref().and_then(|p| diaglog_private_file(p).ok());
        let two = |f: &Option<std::fs::File>| -> Box<dyn std::io::Write + Send> {
            match f.as_ref().and_then(|f| f.try_clone().ok()) {
                Some(f) => Box::new(f),
                None => Box::new(std::io::sink()),
            }
        };
        let (sink, sink2) = (two(&file), two(&file));
        launch::run_masked(&mut cmd, sink, sink2)
    } else {
        launch::run_masked(&mut cmd, std::io::stdout(), std::io::stderr())
    }
    .map_err(|e| {
        if let Some(l) = diag_log.as_mut() {
            l.event(now, &format!("the game could not be started: {e}"));
        }
        LaunchError::Io(e, format!("cannot start {:?}", cmd.get_program()))
    })?;
    if let Some(l) = diag_log.as_mut() {
        let end = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(now);
        l.event(end, &format!("the game ended after {} s: {}", started.elapsed().as_secs(), describe_exit(&status)));
    }
    Ok(match status.code() {
        Some(c) => ExitCode::from(c.clamp(0, 255) as u8),
        None => ExitCode::from(1),
    })
}

fn main() -> ExitCode {
    match run() {
        Ok(c) => c,
        Err(LaunchError::Usage(m)) => {
            eprintln!("{m}");
            ExitCode::from(64)
        }
        Err(e) => {
            eprintln!("tsw-launch: {e}");
            ExitCode::from(1)
        }
    }
}

#[cfg(test)]
mod record_tests {
    use super::*;

    #[test]
    fn the_patcher_key_is_masked_in_the_start_record_and_other_arguments_are_kept() {
        let a = mask_flag(vec!["-noconsole".into(), "-key".into(), "00112233aabbccdd".into(), "-novideo".into()], "-key");
        assert_eq!(a, vec!["-noconsole", "-key", "<redacted>", "-novideo"]);
        assert_eq!(mask_flag(vec!["-novideo".into()], "-key"), vec!["-novideo"]);
    }
}
