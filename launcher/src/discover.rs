//! Automatic detection of the client installation, the Wine/Proton runtime and the managed prefix.
//!
//! One implementation for the CLI, the GUI and the prefix manager, so they cannot disagree. Priority for every value:
//! 1. an explicit key in the configuration file (advanced use),
//! 2. the internally cached result of an earlier detection (`detected` next to the configuration), if still valid,
//! 3. a fresh detection.
//!
//! What is searched (all relative to the process environment, nothing machine-specific is compiled in):
//! * Client: a project checkout's private test copy `analysis/runtime/tsw-copy/TheSecretWorldDX11.exe`, found by walking
//!   up from the launcher's own directory and the working directory (or `$TSW_LAUNCH_ROOT`). Production installations
//!   (Lutris, Steam, ...) are **never** searched, so automatic detection cannot touch them.
//! * Runtime: Steam compatibility tools (`compatibilitytools.d/*`) and Steam's `Proton*` directories that ship the DXVK
//!   and vkd3d components the prefix manager installs. Lutris runner directories are not searched. A runtime listed
//!   in [`VERIFIED_RUNTIMES`] (tested with the client) is preferred over a newer, untested one.
//! * Prefix: the managed prefix `prefix-managed` next to the test copy (inside the project), else
//!   `<data dir>/tsw-launch/prefix`. Existing foreign prefixes are never adopted automatically.

use std::path::{Path, PathBuf};

use crate::config::{self, Config};
use crate::prefix;

pub const CLIENT_EXE: &str = "TheSecretWorldDX11.exe";
/// Directory of the private test copy below a project root, as documented in the project handover.
const INSTALL_DIR: [&str; 3] = ["analysis", "runtime", "tsw-copy"];
const MANAGED_PREFIX: &str = "prefix-managed";
/// Runtimes (directory name prefix) that were actually run with this client.
pub const VERIFIED_RUNTIMES: &[&str] = &["GE-Proton11-11"];

/// The process environment, injectable for tests.
#[derive(Clone, Debug, Default)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_data: Option<PathBuf>,
    pub cwd: Option<PathBuf>,
    pub exe_dir: Option<PathBuf>,
    pub root_override: Option<PathBuf>,
    /// `WINEPREFIX` from the environment: an explicit choice of the caller, ranked below the configuration file.
    pub wineprefix: Option<PathBuf>,
    /// The installation folder chosen in the installation manager (`<base>/game`, `<base>/prefix`), if any.
    pub install_base: Option<PathBuf>,
}

impl Env {
    pub fn from_process() -> Env {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
        Env {
            home: var("HOME"),
            xdg_data: var("XDG_DATA_HOME"),
            cwd: std::env::current_dir().ok(),
            exe_dir: std::env::current_exe().ok().and_then(|e| e.parent().map(Path::to_path_buf)),
            root_override: var("TSW_LAUNCH_ROOT"),
            wineprefix: var("WINEPREFIX"),
            install_base: crate::install::state_path().and_then(|p| crate::install::State::load(&p).ok()).and_then(|s| s.base),
        }
    }

    /// `$XDG_DATA_HOME` (inside a Flatpak: the app's own data folder) or `~/.local/share`.
    pub fn data_dir(&self) -> Option<PathBuf> {
        self.xdg_data.clone().or_else(|| self.home.as_ref().map(|h| h.join(".local").join("share")))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Runtime {
    pub wine: PathBuf,
    pub name: String,
    pub verified: bool,
    rank: Vec<u32>,
}

/// Why detection did not produce a complete result. [`Problem::message`] is meant for users.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Problem {
    NoInstall,
    AmbiguousInstall(Vec<PathBuf>),
    NoRuntime,
}

impl Problem {
    pub fn message(&self) -> String {
        match self {
            Problem::NoInstall => "Es wurde keine TSW-Installation gefunden. Richten Sie sie in der erweiterten Ansicht unter INSTALLATION ein, \
übernehmen Sie eine vorhandene Installation oder geben Sie den Client dort an (CLI: tsw-launch install status)."
                .into(),
            Problem::AmbiguousInstall(v) => format!(
                "Es wurden mehrere TSW-Testinstallationen gefunden ({}). Bitte wählen Sie in der erweiterten Ansicht eine aus.",
                v.iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
            ),
            Problem::NoRuntime => "Es wurde keine passende Wine-/Proton-Laufzeit gefunden (gesucht: Steam-Kompatibilitätstools mit DXVK und vkd3d, \
zum Beispiel GE-Proton). Installieren Sie eine oder geben Sie das Wine-Programm in der erweiterten Ansicht an."
                .into(),
        }
    }
}

/// Result of a detection: everything needed to start, plus plain-language notes (not errors).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Detected {
    pub client_exe: PathBuf,
    pub runner: String,
    pub prefix: PathBuf,
    /// True if the prefix is the launcher-managed one (it may be created and refreshed automatically).
    pub managed_prefix: bool,
    pub notes: Vec<String>,
    /// The same notes as stable `(code, args)` pairs, in the same order, for front ends that translate them.
    pub note_codes: Vec<(&'static str, Vec<String>)>,
}

fn canon(p: &Path) -> PathBuf {
    std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
}

fn exe_of(root: &Path) -> PathBuf {
    INSTALL_DIR.iter().fold(root.to_path_buf(), |p, s| p.join(s)).join(CLIENT_EXE)
}

/// All distinct client executables of project test copies reachable from the environment.
pub fn find_installs(env: &Env) -> Vec<PathBuf> {
    let mut starts: Vec<PathBuf> = Vec::new();
    starts.extend(env.root_override.iter().cloned());
    for base in [&env.exe_dir, &env.cwd].into_iter().flatten() {
        starts.extend(base.ancestors().map(Path::to_path_buf));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    if let Some(f) = env.install_base.as_ref().and_then(|b| crate::install::find_installation(&crate::install::Layout { base: b.clone() }, &Default::default())).filter(|f| f.complete) {
        found.push(canon(&f.client));
    }
    for dir in starts {
        let exe = exe_of(&dir);
        if exe.is_file() {
            let c = canon(&exe);
            if !found.contains(&c) {
                found.push(c);
            }
        }
    }
    found
}

fn version_rank(name: &str) -> Vec<u32> {
    let name = name.strip_suffix("-x86_64").unwrap_or(name);
    let mut out = Vec::new();
    let mut cur = String::new();
    for ch in name.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() {
            cur.push(ch);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
        }
    }
    out
}

fn runtime_at(dir: &Path) -> Option<Runtime> {
    let wine = dir.join("files").join("bin").join("wine");
    if !wine.is_file() {
        return None;
    }
    let rt = prefix::runtime_files_dir(&wine)?;
    // The runtime must ship exactly what the prefix manager installs.
    if !prefix::plan_components(&rt, Path::new("/")).iter().all(|op| op.from.is_file()) {
        return None;
    }
    let name = dir.file_name()?.to_string_lossy().into_owned();
    Some(Runtime { verified: VERIFIED_RUNTIMES.iter().any(|v| name.starts_with(v)), rank: version_rank(&name), name, wine })
}

/// All usable runtimes, best first: verified before unverified, then higher version first.
pub fn find_runtimes(env: &Env) -> Vec<Runtime> {
    fn children(parent: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(parent).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
    }
    let mut dirs: Vec<PathBuf> = Vec::new();
    if let Some(data) = env.data_dir() {
        dirs.extend(children(&data.join("Steam").join("compatibilitytools.d")));
        dirs.extend(children(&data.join("Steam").join("steamapps").join("common")).into_iter().filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().starts_with("Proton"))));
    }
    // the launcher's own runtime folder: <installation base>/runtime (default ~/Games/The Secret World/runtime; the Flatpak is granted exactly this folder)
    if let Some(base) = env.install_base.clone().or_else(|| env.home.as_ref().and_then(|h| crate::install::default_base(Some(h)))) {
        dirs.extend(children(&base.join("runtime")));
    }
    if let Some(home) = &env.home {
        // The host's Steam tools. Inside a Flatpak `XDG_DATA_HOME` points to the app's own data folder, so the host path is
        // spelled out here; it is visible only if the user granted `--filesystem=xdg-data/Steam:ro` (see documentation/packaging.md).
        dirs.extend(children(&home.join(".local").join("share").join("Steam").join("compatibilitytools.d")));
        dirs.extend(children(&home.join(".steam").join("root").join("compatibilitytools.d")));
        dirs.extend(children(&home.join(".steam").join("steam").join("compatibilitytools.d")));
    }
    let mut seen: Vec<PathBuf> = Vec::new();
    let mut out: Vec<Runtime> = Vec::new();
    for d in dirs {
        let Some(r) = runtime_at(&d) else { continue };
        let c = canon(&r.wine);
        if !seen.contains(&c) {
            seen.push(c);
            out.push(r);
        }
    }
    out.sort_by(|a, b| b.verified.cmp(&a.verified).then_with(|| b.rank.cmp(&a.rank)).then_with(|| a.name.cmp(&b.name)));
    out
}

/// The managed prefix belonging to a client: next to the test copy, else below the user data directory.
pub fn managed_prefix(env: &Env, client_exe: &Path) -> Option<PathBuf> {
    if let Some(base) = env.install_base.as_ref().filter(|b| canon(client_exe).starts_with(canon(b))) {
        return Some(base.join("prefix"));
    }
    let dir = client_exe.parent()?;
    if dir.file_name().is_some_and(|n| n == INSTALL_DIR[2]) {
        return dir.parent().map(|runtime| runtime.join(MANAGED_PREFIX));
    }
    env.data_dir().map(|d| d.join("tsw-launch").join("prefix"))
}

/// Where the internal detection cache lives (next to the configuration file).
pub fn cache_path() -> Option<PathBuf> {
    config::write_path().and_then(|p| p.parent().map(|d| d.join("detected")))
}

fn load_cache(path: &Path) -> Config {
    config::load(path).unwrap_or_default()
}

/// Detect (or take from configuration/cache) everything needed to start. `cfg` holds explicit overrides.
pub fn detect(env: &Env, cfg: &Config, cache: &Config) -> Result<Detected, Problem> {
    let mut notes = Vec::new();
    let mut note_codes: Vec<(&'static str, Vec<String>)> = Vec::new();

    let client_exe = match (&cfg.client_exe, &cache.client_exe) {
        (Some(p), _) => p.clone(),
        (None, Some(c)) if c.is_file() && !crate::winnative::is_excluded_now(c) => c.clone(),
        _ => {
            let mut v = find_installs(env);
            if v.is_empty() && cfg!(windows) {
                // native Windows: the usual places of the original installer and of Steam
                v = find_native_installs(&crate::winnative::search_roots());
            }
            match v.len() {
                0 => return Err(Problem::NoInstall),
                1 => v.remove(0),
                _ => return Err(Problem::AmbiguousInstall(v)),
            }
        }
    };

    let runner = match (&cfg.runner, &cache.runner) {
        (Some(r), _) => r.clone(),
        (None, Some(c)) if Path::new(c).is_file() => c.clone(),
        _ if cfg!(windows) => "native".to_string(), // Windows runs the client itself: no Wine runtime exists to be searched
        _ => {
            let runtimes = find_runtimes(env);
            let best = runtimes.first().ok_or(Problem::NoRuntime)?;
            if runtimes.len() > 1 {
                notes.push(format!("Mehrere Laufzeiten gefunden, verwendet wird {}.", best.name));
                note_codes.push(("discover.many_runtimes", vec![best.name.clone()]));
            }
            if !best.verified {
                notes.push(format!("Die Laufzeit {} wurde mit diesem Client noch nicht getestet.", best.name));
                note_codes.push(("discover.untested_runtime", vec![best.name.clone()]));
            }
            best.wine.display().to_string()
        }
    };

    // The native runner has no Wine prefix. `Detected.prefix` is not optional, so the client's own folder stands in (nothing is ever created or written there
    // because of this); it is not a managed prefix.
    let native = runner == "native";
    let (prefix, managed_prefix) = match (&cfg.prefix, &env.wineprefix, &cache.prefix) {
        _ if native => (client_exe.parent().map(Path::to_path_buf).unwrap_or_default(), false),
        (Some(p), _, _) => (p.clone(), false),
        (None, Some(p), _) => (p.clone(), false),
        (None, None, Some(c)) => (c.clone(), true),
        (None, None, None) => (self::managed_prefix(env, &client_exe).ok_or(Problem::NoInstall)?, true),
    };
    Ok(Detected { client_exe, runner, prefix, managed_prefix, notes, note_codes })
}

/// Client executables below the given folders (the default places of the original installer and of Steam on Windows), each found once, in order.
pub fn find_native_installs(roots: &[PathBuf]) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for root in roots {
        if let Some(exe) = crate::install::find_client(root) {
            if !out.contains(&exe) {
                out.push(exe);
            }
        }
    }
    out
}

/// [`detect`] with the cache file: loads it, detects, and stores the result when it changed (failure to store is ignored;
/// the cache is only an optimisation and must never make a start fail).
pub fn resolve(env: &Env, cfg: &Config, cache_file: Option<&Path>) -> Result<Detected, Problem> {
    let cache = cache_file.map(load_cache).unwrap_or_default();
    let d = detect(env, cfg, &cache)?;
    if let Some(path) = cache_file {
        let fresh = Config { client_exe: Some(d.client_exe.clone()), runner: Some(d.runner.clone()), prefix: d.managed_prefix.then(|| d.prefix.clone()), ..Config::default() };
        let stored = Config { client_exe: cache.client_exe.clone(), runner: cache.runner.clone(), prefix: cache.prefix.clone(), ..Config::default() };
        if cfg.client_exe.is_none() && cfg.runner.is_none() && fresh != stored {
            let _ = config::save(path, &fresh);
        }
    }
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-disc-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn fake_project(root: &Path) -> PathBuf {
        let exe = exe_of(root);
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        exe
    }

    fn fake_runtime(data: &Path, name: &str) -> PathBuf {
        let dir = data.join("Steam").join("compatibilitytools.d").join(name);
        let files = dir.join("files");
        std::fs::create_dir_all(files.join("bin")).unwrap();
        std::fs::create_dir_all(files.join("lib").join("wine")).unwrap();
        std::fs::write(files.join("bin").join("wine"), b"").unwrap();
        for op in prefix::plan_components(&files, Path::new("/")) {
            std::fs::create_dir_all(op.from.parent().unwrap()).unwrap();
            std::fs::write(&op.from, b"x").unwrap();
        }
        files.join("bin").join("wine")
    }

    fn env_for(home: &Path, cwd: &Path) -> Env {
        Env { home: Some(home.to_path_buf()), xdg_data: Some(home.join(".local/share")), cwd: Some(cwd.to_path_buf()), exe_dir: None, root_override: None, wineprefix: None, install_base: None }
    }

    #[test]
    fn the_install_is_found_from_any_subdirectory_of_the_project() {
        let d = tmp("inst");
        let exe = fake_project(&d.join("proj"));
        let sub = d.join("proj").join("launcher-gui").join("target").join("release");
        std::fs::create_dir_all(&sub).unwrap();
        let env = env_for(&d.join("home"), &sub);
        assert_eq!(find_installs(&env), vec![canon(&exe)]);
        assert!(find_installs(&env_for(&d.join("home"), &d)).is_empty(), "nothing found outside a project");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn the_native_runner_needs_no_wine_runtime_and_no_prefix() {
        let d = tmp("native");
        let exe = fake_project(&d.join("game"));
        let cfg = Config { client_exe: Some(exe.clone()), runner: Some("native".into()), ..Config::default() };
        let found = detect(&env_for(&d.join("home"), &d), &cfg, &Config::default()).expect("no runtime is searched for the native runner");
        assert_eq!(found.runner, "native");
        assert_eq!(found.prefix, exe.parent().unwrap(), "the client folder stands in for the missing prefix");
        assert!(!found.managed_prefix);
        // the Wine runner still insists on a runtime
        let wine = Config { client_exe: Some(exe), ..Config::default() };
        assert!(matches!(detect(&env_for(&d.join("home"), &d), &wine, &Config::default()), Err(Problem::NoRuntime)));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn native_installs_are_found_in_the_given_default_places_once_each() {
        let d = tmp("nativeroots");
        let a = d.join("Program Files (x86)").join("Funcom");
        let exe = exe_of(&a.join("The Secret World"));
        std::fs::create_dir_all(exe.parent().unwrap()).unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        let found = find_native_installs(&[a.clone(), d.join("missing"), a.clone()]);
        assert_eq!(found, vec![exe]);
        assert!(find_native_installs(&[d.join("nothing")]).is_empty());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn two_projects_are_reported_as_ambiguous_and_never_guessed() {
        let d = tmp("amb");
        fake_project(&d.join("a"));
        fake_project(&d.join("b"));
        let mut env = env_for(&d.join("home"), &d.join("a"));
        env.exe_dir = Some(d.join("b"));
        let err = detect(&env, &Config::default(), &Config::default()).unwrap_err();
        assert!(matches!(err, Problem::AmbiguousInstall(ref v) if v.len() == 2));
        assert!(err.message().contains("mehrere"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_verified_runtime_beats_a_newer_untested_one_and_incomplete_ones_are_skipped() {
        let d = tmp("rt");
        let data = d.join("home").join(".local/share");
        fake_runtime(&data, "GE-Proton11-11-x86_64");
        fake_runtime(&data, "GE-Proton12-3-x86_64");
        let broken = data.join("Steam/compatibilitytools.d/GE-Proton99-1/files/bin");
        std::fs::create_dir_all(&broken).unwrap();
        std::fs::write(broken.join("wine"), b"").unwrap();
        let rts = find_runtimes(&env_for(&d.join("home"), &d));
        assert_eq!(rts.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(), ["GE-Proton11-11-x86_64", "GE-Proton12-3-x86_64"]);
        assert!(rts[0].verified && !rts[1].verified);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn full_detection_uses_the_managed_prefix_inside_the_project_and_explains_choices() {
        let d = tmp("full");
        let exe = fake_project(&d.join("proj"));
        let data = d.join("home").join(".local/share");
        fake_runtime(&data, "GE-Proton12-3-x86_64");
        let got = detect(&env_for(&d.join("home"), &d.join("proj")), &Config::default(), &Config::default()).unwrap();
        assert_eq!(got.client_exe, canon(&exe));
        assert!(got.prefix.ends_with("analysis/runtime/prefix-managed") && got.managed_prefix);
        assert!(got.notes.iter().any(|n| n.contains("noch nicht getestet")));
        assert!(got.note_codes.iter().any(|(c, a)| *c == "discover.untested_runtime" && a.len() == 1), "the same note is also available as a code");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn explicit_configuration_wins_and_stale_cache_is_ignored() {
        let d = tmp("prio");
        let exe = fake_project(&d.join("proj"));
        let env = env_for(&d.join("home"), &d.join("proj"));
        fake_runtime(&d.join("home").join(".local/share"), "GE-Proton11-11-x86_64");
        let explicit = Config { runner: Some("/opt/wine".into()), prefix: Some("/p".into()), ..Config::default() };
        let got = detect(&env, &explicit, &Config::default()).unwrap();
        assert_eq!((got.runner.as_str(), got.prefix.as_path(), got.managed_prefix), ("/opt/wine", Path::new("/p"), false));
        let stale = Config { client_exe: Some(d.join("gone.exe")), runner: Some(d.join("gone-wine").display().to_string()), ..Config::default() };
        let got = detect(&env, &Config::default(), &stale).unwrap();
        assert_eq!(got.client_exe, canon(&exe));
        assert!(got.runner.ends_with("GE-Proton11-11-x86_64/files/bin/wine"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn wineprefix_from_the_environment_counts_as_explicit() {
        let d = tmp("wpfx");
        fake_project(&d.join("proj"));
        fake_runtime(&d.join("home").join(".local/share"), "GE-Proton11-11-x86_64");
        let mut env = env_for(&d.join("home"), &d.join("proj"));
        env.wineprefix = Some("/somewhere/pfx".into());
        let got = detect(&env, &Config::default(), &Config { prefix: Some("/cached".into()), ..Config::default() }).unwrap();
        assert_eq!((got.prefix.as_path(), got.managed_prefix), (Path::new("/somewhere/pfx"), false));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn nothing_found_gives_understandable_messages() {
        let d = tmp("none");
        let env = env_for(&d.join("home"), &d);
        assert_eq!(detect(&env, &Config::default(), &Config::default()).unwrap_err(), Problem::NoInstall);
        fake_project(&d.join("proj"));
        let env = env_for(&d.join("home"), &d.join("proj"));
        let e = detect(&env, &Config::default(), &Config::default()).unwrap_err();
        assert_eq!(e, Problem::NoRuntime);
        assert!(e.message().contains("Wine-/Proton-Laufzeit"));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn the_cache_is_written_once_and_never_overrides_explicit_values() {
        let d = tmp("cache");
        fake_project(&d.join("proj"));
        fake_runtime(&d.join("home").join(".local/share"), "GE-Proton11-11-x86_64");
        let env = env_for(&d.join("home"), &d.join("proj"));
        let cache = d.join("cfg").join("detected");
        let a = resolve(&env, &Config::default(), Some(&cache)).unwrap();
        let stored = config::load(&cache).unwrap();
        assert_eq!(stored.client_exe.as_deref(), Some(a.client_exe.as_path()));
        assert!(stored.prefix.is_some());
        let explicit = Config { client_exe: Some("/x/y.exe".into()), ..Config::default() };
        let before = std::fs::read_to_string(&cache).unwrap();
        resolve(&env, &explicit, Some(&cache)).unwrap();
        assert_eq!(std::fs::read_to_string(&cache).unwrap(), before);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn no_machine_specific_absolute_paths_are_compiled_in() {
        let src = include_str!("discover.rs");
        let code = src.split("#[cfg(test)]").next().unwrap();
        for forbidden in ["/home/", "/usr/", "/opt/", "C:\\"] {
            assert!(!code.contains(forbidden), "absolute path {forbidden:?} in source");
        }
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn an_installation_made_by_the_installation_manager_is_found_with_its_own_prefix() {
        let d = std::env::temp_dir().join(format!("tsw-discover-base-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let base = d.join("Games").join("TheSecretWorld");
        let exe = base.join("prefix/drive_c/Program Files (x86)/Funcom/The Secret World").join(CLIENT_EXE);
        std::fs::create_dir_all(exe.parent().unwrap().join("RDB")).unwrap();
        std::fs::write(exe.parent().unwrap().join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        let env = Env { install_base: Some(base.clone()), ..Env::default() };
        assert_eq!(find_installs(&env), vec![canon(&exe)]);
        assert_eq!(managed_prefix(&env, &canon(&exe)), Some(base.join("prefix")));
        assert_eq!(find_installs(&Env::default()), Vec::<PathBuf>::new(), "without a chosen base nothing is found by this route");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)] // Wine runtime and prefix discovery; Windows has no Wine (detect() picks the native runner, tested in native tests)
    #[test]
    fn runtimes_are_found_in_the_launchers_own_folder_and_at_the_host_steam_path_without_duplicates() {
        let d = std::env::temp_dir().join(format!("tsw-discover-rt-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let (home, sandbox_data) = (d.join("home"), d.join("home/.var/app/org.tswlauncher.Launcher/data"));
        let make = |dir: &Path| {
            let files = dir.join("files");
            std::fs::create_dir_all(files.join("bin")).unwrap();
            std::fs::write(files.join("bin").join("wine"), b"x").unwrap();
            for op in prefix::plan_components(&files, Path::new("/")) {
                std::fs::create_dir_all(op.from.parent().unwrap()).unwrap();
                std::fs::write(&op.from, b"dll").unwrap();
            }
        };
        make(&home.join(".local/share/Steam/compatibilitytools.d/GE-Proton11-11"));
        make(&home.join("Games/The Secret World/runtime/GE-Proton12-1"));
        let env = Env { home: Some(home.clone()), xdg_data: Some(sandbox_data.clone()), ..Env::default() };
        let names: Vec<_> = find_runtimes(&env).into_iter().map(|r| r.name).collect();
        assert_eq!(names, ["GE-Proton11-11", "GE-Proton12-1"], "verified first; the launcher's runtime folder (~/Games/The Secret World/runtime) and the host Steam folder are searched");
        // not in a sandbox: the same Steam folder reached through XDG_DATA_HOME and through HOME is one runtime, not two
        let plain = Env { home: Some(home.clone()), xdg_data: Some(home.join(".local/share")), ..Env::default() };
        assert_eq!(find_runtimes(&plain).len(), 2, "the Steam runtime once (HOME and XDG_DATA_HOME name the same folder) plus the launcher's own runtime folder");
        let _ = std::fs::remove_dir_all(&d);
    }
}
