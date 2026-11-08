//! Native Windows support: where things live, how programs are started, how Funcom's processes are found, and which features exist at all.
//!
//! Everything that decides something is a PURE function fed by an environment lookup or by text, so it is tested on Linux as well; only the thin
//! launchers at the end (`list_processes`, `start_elevated`) touch the operating system, and only under `cfg(windows)`. The only `unsafe` code is the Known Folder
//! call in `roaming_appdata`. Otherwise:
//! processes are listed with Windows PowerShell and elevating programs are started through `cmd /C start` (ShellExecute), which shows the Windows
//! UAC prompt itself. The launcher never runs elevated.
//!
//! Windows has no Wine prefix, no Proton runtime, no Gamescope and no DXVK: the original client is started directly and Direct3D is the system's.
//! The Microsoft components of the Linux setup are therefore NOT downloaded or installed here (see [`directx_runtime`]).

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// An environment lookup; injected so that the logic can be tested without the real environment. Empty values count as unset.
pub type Env<'a> = &'a dyn Fn(&str) -> Option<String>;

/// The real process environment.
pub fn real_env(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

/// Folder name of the launcher below `%APPDATA%` (settings) and `%LOCALAPPDATA%` (downloads, caches, logs). Windows folder names are case-insensitive,
/// so an earlier `TSW-Forever` folder below `%LOCALAPPDATA%` is the same folder.
pub const APP_DIR: &str = "tsw-forever";
/// Folder of the first Windows versions of the launcher below `%APPDATA%` (the Linux name); its files are copied to the new folder, never removed.
pub const LEGACY_CONFIG_DIR: &str = "tsw-launch";

/// What exists on a platform. The front ends show or hide controls from this; nothing is "shown but without function".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// A Wine/Proton runtime with a prefix is needed and managed (download, prefix, Direct3D components).
    pub wine_runtime: bool,
    /// Gamescope window mode.
    pub gamescope: bool,
    /// Frame-rate limit and counter through DXVK.
    pub fps_limit: bool,
    /// A private Wine desktop of a chosen size.
    pub wine_desktop: bool,
    /// Downloading and installing Microsoft SDK/runtime DLLs for the game (with the user's consent).
    pub microsoft_components: bool,
    /// Update check against the Flatpak repository and the file portal.
    pub flatpak: bool,
    /// The screen resolution can be chosen (written to the game's own preferences).
    pub resolution: bool,
    /// Window/full-screen mode can be chosen (game preferences).
    pub display_mode: bool,
    /// Diagnostics for 32-bit libraries and Vulkan drivers (`doctor`).
    pub linux_doctor: bool,
}

impl Capabilities {
    pub const LINUX: Capabilities =
        Capabilities { wine_runtime: true, gamescope: true, fps_limit: true, wine_desktop: true, microsoft_components: true, flatpak: true, resolution: true, display_mode: true, linux_doctor: true };
    pub const WINDOWS: Capabilities = Capabilities {
        wine_runtime: false,
        gamescope: false,
        fps_limit: false,
        wine_desktop: false,
        microsoft_components: false,
        flatpak: false,
        resolution: true,
        display_mode: true,
        linux_doctor: false,
    };

    /// The capabilities of the platform this binary was compiled for.
    pub fn current() -> Capabilities {
        if cfg!(windows) {
            Capabilities::WINDOWS
        } else {
            Capabilities::LINUX
        }
    }
}

// ---- locations ----------------------------------------------------------------------------------------------------------

/// `%LOCALAPPDATA%\tsw-forever`: the launcher's per-user data (downloaded installer, caches, logs). Nothing here needs administrator rights.
pub fn data_dir(env: Env) -> Option<PathBuf> {
    env("LOCALAPPDATA").map(|d| PathBuf::from(d).join(APP_DIR))
}

/// The user's roaming application data folder (`FOLDERID_RoamingAppData`, normally `C:\Users\<name>\AppData\Roaming`), asked from Windows through the
/// Known Folder API - not read from an environment variable, which a program or a user can change. `None` if Windows does not answer.
#[cfg(windows)]
pub fn roaming_appdata() -> Option<PathBuf> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStringExt;
    #[repr(C)]
    struct Guid {
        data1: u32,
        data2: u16,
        data3: u16,
        data4: [u8; 8],
    }
    // FOLDERID_RoamingAppData = {3EB685DB-65F9-4CF6-A03A-E3EF65729F3D}
    static ROAMING_APP_DATA: Guid = Guid { data1: 0x3EB6_85DB, data2: 0x65F9, data3: 0x4CF6, data4: [0xA0, 0x3A, 0xE3, 0xEF, 0x65, 0x72, 0x9F, 0x3D] };
    #[link(name = "shell32")]
    extern "system" {
        fn SHGetKnownFolderPath(rfid: *const Guid, flags: u32, token: *mut c_void, path: *mut *mut u16) -> i32;
    }
    #[link(name = "ole32")]
    extern "system" {
        fn CoTaskMemFree(memory: *mut c_void);
    }
    let mut raw: *mut u16 = std::ptr::null_mut();
    // SAFETY: the GUID outlives the call; `raw` receives either null or a NUL-terminated UTF-16 string allocated by Windows, which is copied and then
    // released with CoTaskMemFree exactly once (also when the call reports an error, as the documentation demands).
    unsafe {
        let hr = SHGetKnownFolderPath(&ROAMING_APP_DATA, 0, std::ptr::null_mut(), &mut raw);
        let result = if hr >= 0 && !raw.is_null() {
            let mut len = 0;
            while *raw.add(len) != 0 {
                len += 1;
            }
            Some(PathBuf::from(std::ffi::OsString::from_wide(std::slice::from_raw_parts(raw, len))))
        } else {
            None
        };
        if !raw.is_null() {
            CoTaskMemFree(raw as *mut c_void);
        }
        result.filter(|p| p.is_absolute())
    }
}

/// Not Windows: there is no such folder.
#[cfg(not(windows))]
pub fn roaming_appdata() -> Option<PathBuf> {
    None
}

/// `%APPDATA%\tsw-forever`: the launcher's settings (the chosen game folder, the launcher options, the installation state). The roaming folder comes
/// from Windows (Known Folder); the environment variable is only the fallback when Windows gives no answer.
pub fn config_dir(roaming: Option<PathBuf>, env: Env) -> Option<PathBuf> {
    roaming.or_else(|| env("APPDATA").map(PathBuf::from)).map(|d| d.join(APP_DIR))
}

/// The configuration file: `<config_dir>\config`.
pub fn config_file(roaming: Option<PathBuf>, env: Env) -> Option<PathBuf> {
    config_dir(roaming, env).map(|d| d.join("config"))
}

/// Copy the settings of an earlier folder (`%APPDATA%\tsw-launch`: `config`, `install-state`, any other regular file) into the new one. Rules: a file that
/// exists in the new folder is NEVER overwritten; nothing is deleted or moved (the old folder stays as it was, so an older version still works and no user
/// data can be lost); sub-folders and links are not followed. Returns the files that were copied.
pub fn migrate_legacy_settings(new_dir: &Path, old_dir: &Path) -> std::io::Result<Vec<PathBuf>> {
    let mut copied = Vec::new();
    if new_dir == old_dir || !old_dir.is_dir() {
        return Ok(copied);
    }
    std::fs::create_dir_all(new_dir)?;
    for entry in std::fs::read_dir(old_dir)? {
        let entry = entry?;
        let kind = entry.file_type()?;
        let target = new_dir.join(entry.file_name());
        if kind.is_file() && !target.exists() {
            std::fs::copy(entry.path(), &target)?;
            copied.push(target);
        }
    }
    Ok(copied)
}

/// The settings folder of this user on this machine, with the one-time migration from the earlier folder. Called by [`crate::config::write_path`].
#[cfg(windows)]
pub fn settings_dir_migrated() -> Option<PathBuf> {
    static ONCE: std::sync::Once = std::sync::Once::new();
    let roaming = roaming_appdata();
    let dir = config_dir(roaming.clone(), &real_env)?;
    ONCE.call_once(|| {
        if let Some(base) = roaming.or_else(|| real_env("APPDATA").map(PathBuf::from)) {
            // best effort: a failed copy must not stop the launcher; it then starts with empty settings and the old files are still there
            let _ = migrate_legacy_settings(&dir, &base.join(LEGACY_CONFIG_DIR));
        }
    });
    Some(dir)
}

/// Where the ORIGINAL Funcom installer (an Inno Setup program) records the folder the user chose: the uninstall key of the game, in the 32-bit view of
/// `HKEY_LOCAL_MACHINE` (observed on Windows 11 with the installer `TheSecretWorldInstaller.exe`: `Inno Setup: App Path` and `InstallLocation`, both the
/// chosen folder). The other views and `HKEY_CURRENT_USER` are looked at too, for an installation made without administrator rights.
pub const GAME_UNINSTALL_KEY: &str = r"SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\The Secret World_is1";

/// The PowerShell script that prints the recorded installation folders, one line per value, each as the hexadecimal UTF-8 bytes of the path (so that umlauts
/// and other characters survive the console's code page).
pub fn registry_script() -> String {
    format!(
        "$ErrorActionPreference='SilentlyContinue'; foreach($v in 'Registry32','Registry64'){{ foreach($h in 'LocalMachine','CurrentUser'){{ \
         $k=[Microsoft.Win32.RegistryKey]::OpenBaseKey($h,$v).OpenSubKey('{GAME_UNINSTALL_KEY}'); \
         if($k){{ foreach($n in 'Inno Setup: App Path','InstallLocation'){{ $p=$k.GetValue($n); if($p){{ -join ([Text.Encoding]::UTF8.GetBytes([string]$p) | ForEach-Object {{ $_.ToString('x2') }}) }} }} }} }} }}"
    )
}

/// Parse the output of [`registry_script`]: absolute Windows paths without a trailing separator, in the order given, without duplicates. Anything else
/// (garbage, relative paths, odd hexadecimal) is ignored.
pub fn parse_registered_dirs(text: &str) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    for line in text.lines() {
        let hex = line.trim().trim_start_matches('\u{feff}');
        if hex.is_empty() || hex.len() % 2 != 0 || !hex.is_ascii() {
            continue;
        }
        let bytes: Option<Vec<u8>> = (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect();
        let Some(Ok(path)) = bytes.map(String::from_utf8) else { continue };
        let path = path.trim().trim_end_matches(['\\', '/']);
        if is_windows_absolute(path) && !path.contains('\0') && !out.iter().any(|p| p.as_os_str().eq_ignore_ascii_case(path)) {
            out.push(PathBuf::from(path));
        }
    }
    out
}

/// The installation folders the original installer registered (see [`GAME_UNINSTALL_KEY`]). Empty when there is none or when PowerShell cannot be asked.
/// The answer is remembered for a few seconds: the window asks on every refresh, and a PowerShell start is not free.
#[cfg(windows)]
pub fn registered_game_dirs() -> Vec<PathBuf> {
    use std::os::windows::process::CommandExt;
    use std::sync::Mutex;
    // the tests of this project run on machines that may have a real installation registered: they switch the system search off
    if real_env("TSW_LAUNCH_NO_REGISTRY").is_some() || real_env("TSW_LAUNCH_NO_SYSTEM_SEARCH").is_some() {
        return Vec::new();
    }
    static CACHE: Mutex<Option<(std::time::Instant, Vec<PathBuf>)>> = Mutex::new(None);
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, dirs)) = cache.as_ref() {
        if at.elapsed() < Duration::from_secs(5) {
            return dirs.clone();
        }
    }
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &registry_script()]).stdin(std::process::Stdio::null());
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    let dirs = match cmd.output() {
        Ok(out) if out.status.success() => parse_registered_dirs(&String::from_utf8_lossy(&out.stdout)),
        _ => Vec::new(),
    };
    *cache = Some((std::time::Instant::now(), dirs.clone()));
    dirs
}

/// Outside Windows there is no such registry.
#[cfg(not(windows))]
pub fn registered_game_dirs() -> Vec<PathBuf> {
    Vec::new()
}

/// Folders where people put the game when they choose a folder themselves in the original installer, on one drive (`C:`): only these named folders, no
/// search of the whole drive. Used when the registry has no (valid) entry.
pub fn drive_candidates(letter: char) -> Vec<PathBuf> {
    ["Games\\The Secret World", "Games\\TheSecretWorld", "Games\\TSW", "Spiele\\The Secret World", "Funcom\\The Secret World", "The Secret World"]
        .iter()
        .map(|rel| PathBuf::from(format!("{}:\\{rel}", letter.to_ascii_uppercase())))
        .collect()
}

/// The letters of the fixed (internal or permanently attached) drives. Removable and network drives are left out on purpose: asking a disconnected network
/// drive can block for a long time.
#[cfg(windows)]
pub fn fixed_drive_letters() -> Vec<char> {
    #[link(name = "kernel32")]
    extern "system" {
        fn GetLogicalDrives() -> u32;
        fn GetDriveTypeW(root: *const u16) -> u32;
    }
    // SAFETY: GetLogicalDrives takes no arguments; GetDriveTypeW gets a NUL-terminated UTF-16 root path that lives for the call.
    let mask = unsafe { GetLogicalDrives() };
    (0..26u32)
        .filter(|i| mask & (1 << i) != 0)
        .map(|i| (b'A' + i as u8) as char)
        .filter(|c| {
            let root: Vec<u16> = format!("{c}:\\").encode_utf16().chain(std::iter::once(0)).collect();
            unsafe { GetDriveTypeW(root.as_ptr()) == 3 } // DRIVE_FIXED
        })
        .collect()
}

/// Not Windows: no drive letters.
#[cfg(not(windows))]
pub fn fixed_drive_letters() -> Vec<char> {
    Vec::new()
}

// ---- test mode: folders the launcher must not see ---------------------------------------------------------------------------------

/// Names of the environment variables of the isolated first-installation test (documentation/building-windows.md). Normal users never set them, so they
/// have no effect for them.
pub const ENV_EXCLUDE_DIRS: &str = "TSW_LAUNCH_EXCLUDE_DIRS";
/// Simulates missing Direct3D libraries (`all` or a comma list of file names) so that the DirectX help can be tried on a machine that has them.
pub const ENV_TEST_DIRECTX_MISSING: &str = "TSW_LAUNCH_TEST_DIRECTX_MISSING";

/// The user's persisted decision "install a new copy": installations that are ignored from then on, and the folder of the new copy as an extra search root.
/// Set by the front ends from the installation state ([`crate::install::State::apply_search`]); process-wide because the search functions are free functions.
struct UserSearch {
    excluded: Vec<PathBuf>,
    roots: Vec<PathBuf>,
    /// The automatic detection of existing installations is paused (see [`AUTODETECT_PAUSED_ON_WINDOWS`]).
    paused: bool,
    /// While paused: the registry is read again because the launcher started the original installer; only entries that are NEW since then count.
    registry_allowed: bool,
    /// The folders the registry named when the launcher started the installer (they are not "new").
    registry_baseline: Vec<PathBuf>,
    /// While paused: stored installation paths (configuration, installation state, detection cache) count only after a deliberate choice.
    stored_honored: bool,
}

const NO_SEARCH_DECISION: UserSearch = UserSearch { excluded: Vec::new(), roots: Vec::new(), paused: false, registry_allowed: false, registry_baseline: Vec::new(), stored_honored: true };

static USER_SEARCH: std::sync::Mutex<UserSearch> = std::sync::Mutex::new(NO_SEARCH_DECISION);

/// TEMPORARY owner decision (2026-11-08), Windows only: the automatic detection of existing The Secret World installations is switched off so that a complete
/// first installation can be tested on a machine that already has the game. While it is on:
/// - nothing is detected through the registry, the drive search, the default folders (Program Files, Steam) or a stored path of an old configuration;
/// - the first-run choices (download / installer from disk / existing installation) appear at once; choosing an existing installation by hand works;
/// - once the launcher has started the original installer, the registry is read again but only for entries that are NEW since the start, so the new
///   installation is found and used while an old one stays invisible;
/// - a stored path counts as soon as the user chose it deliberately (manual choice or finished installation).
///
/// Set to `false` to bring the automatic detection back.
pub const AUTODETECT_PAUSED_ON_WINDOWS: bool = true;

static PAUSE_ENABLED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// Is the detection pause in force? [`AUTODETECT_PAUSED_ON_WINDOWS`], unless a test switched it off ([`set_autodetect_pause_enabled`]).
pub fn autodetect_pause_enabled() -> bool {
    AUTODETECT_PAUSED_ON_WINDOWS && PAUSE_ENABLED.load(std::sync::atomic::Ordering::SeqCst)
}

/// For tests that are about something else than the pause: switch the pause off (and back on with `true`). The program never calls this.
pub fn set_autodetect_pause_enabled(on: bool) {
    PAUSE_ENABLED.store(on, std::sync::atomic::Ordering::SeqCst);
}

/// Replace the user's search decision (empty lists: nothing is ignored, no extra root). The detection pause is untouched.
pub fn set_user_search(excluded: Vec<PathBuf>, roots: Vec<PathBuf>) {
    let mut u = USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    u.excluded = excluded;
    u.roots = roots;
}

/// Hand the state of the detection pause to the search functions (called with the installation state, see [`crate::install::State::apply_search`]).
pub fn set_autodetect(paused: bool, registry_allowed: bool, registry_baseline: Vec<PathBuf>, stored_honored: bool) {
    let mut u = USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
    u.paused = paused && cfg!(windows);
    u.registry_allowed = registry_allowed;
    u.registry_baseline = registry_baseline;
    u.stored_honored = stored_honored || !u.paused;
}

/// Back to the neutral state (no decision, no pause). Used by tests and when a state is reset.
pub fn reset_user_search() {
    *USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner()) = NO_SEARCH_DECISION;
    PAUSE_ENABLED.store(true, std::sync::atomic::Ordering::SeqCst);
}

/// Is the automatic detection paused right now?
pub fn autodetect_paused() -> bool {
    USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner()).paused
}

/// May a stored installation path (configuration, installation state, detection cache) be used? Always, except while the detection is paused and the user has
/// not chosen an installation deliberately.
pub fn stored_paths_honored() -> bool {
    USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner()).stored_honored
}

/// The registry folders that are new since the baseline (not the same folder, not inside or around one of them). Pure.
pub fn new_registry_dirs(found: Vec<PathBuf>, baseline: &[PathBuf]) -> Vec<PathBuf> {
    found.into_iter().filter(|d| !baseline.iter().any(|b| overlaps(d, b))).collect()
}

/// Every folder the launcher must not see: the test mode's (`TSW_LAUNCH_EXCLUDE_DIRS`) and the ones the user chose to ignore.
pub fn all_excluded_now() -> Vec<PathBuf> {
    let mut v = excluded_dirs(&real_env);
    v.extend(USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner()).excluded.iter().cloned());
    v
}

/// Do two folders overlap (the same folder, or one inside the other)? Whole folder names, case-insensitive, canonical where the folder exists.
pub fn overlaps(a: &Path, b: &Path) -> bool {
    let (a, b) = (comparison_form(a), comparison_form(b));
    !a.is_empty() && !b.is_empty() && (a == b || a.starts_with(&format!("{b}\\")) || b.starts_with(&format!("{a}\\")))
}

/// The folders named in `TSW_LAUNCH_EXCLUDE_DIRS` (separated by `;`). Nothing in them is ever detected, adopted or searched by the launcher. They are only
/// compared by name, never opened, changed or listed.
pub fn excluded_dirs(env: Env) -> Vec<PathBuf> {
    env(ENV_EXCLUDE_DIRS).map(|v| v.split(';').map(str::trim).filter(|s| !s.is_empty()).map(PathBuf::from).collect()).unwrap_or_default()
}

/// A path in comparison form: canonical if it exists (short names, `..` and links resolved), `\` separators, no `\\?\` prefix, no trailing separator, lower case.
fn comparison_form(p: &Path) -> String {
    // the longest existing ancestor is made canonical (short 8.3 names, links, `..`), the rest of a not yet existing path is appended as typed: a folder that
    // does not exist yet must still be recognised as lying inside an existing one
    let mut existing = p.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    let real = loop {
        if let Ok(c) = std::fs::canonicalize(&existing) {
            break rest.iter().rev().fold(c, |acc, part| acc.join(part));
        }
        match (existing.parent().map(Path::to_path_buf), existing.file_name().map(std::ffi::OsString::from)) {
            (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
                rest.push(name);
                existing = parent;
            }
            _ => break p.to_path_buf(),
        }
    };
    let text = real.to_string_lossy().replace('/', "\\");
    text.trim_start_matches("\\\\?\\").trim_end_matches('\\').to_ascii_lowercase()
}

/// Is `path` one of `excluded` or inside one of them?
pub fn is_excluded(path: &Path, excluded: &[PathBuf]) -> bool {
    if excluded.is_empty() {
        return false;
    }
    let p = comparison_form(path);
    excluded.iter().map(|e| comparison_form(e)).any(|e| !e.is_empty() && (p == e || p.starts_with(&format!("{e}\\"))))
}

/// [`is_excluded`] against the test mode's folders and the ones the user chose to ignore.
pub fn is_excluded_now(path: &Path) -> bool {
    is_excluded(path, &all_excluded_now())
}

/// Is the isolated test mode on?
pub fn test_mode_active() -> bool {
    !excluded_dirs(&real_env).is_empty()
}

/// The Direct3D libraries of the DirectX June 2010 runtime that are missing on this computer (empty: all present). `TSW_LAUNCH_TEST_DIRECTX_MISSING` simulates
/// missing ones for a test.
pub fn directx_missing_now() -> Vec<&'static str> {
    if let Some(v) = real_env(ENV_TEST_DIRECTX_MISSING) {
        return DIRECTX_LIBRARIES.iter().copied().filter(|l| v.split(',').any(|x| x.trim().eq_ignore_ascii_case("all") || x.trim().eq_ignore_ascii_case(l))).collect();
    }
    directx_missing(&system_dirs(&real_env), &|p| p.is_file())
}

/// Every folder worth searching for the game on this computer: first what the original installer registered, then the default folders, then the usual
/// self-chosen folders on the fixed drives (only those that exist).
pub fn search_roots() -> Vec<PathBuf> {
    let excluded = all_excluded_now();
    // the folder the user chose for a new copy comes first: the new installation is looked for there before anything else
    let (mut roots, paused, registry_allowed, baseline): (Vec<PathBuf>, bool, bool, Vec<PathBuf>) = {
        let u = USER_SEARCH.lock().unwrap_or_else(|e| e.into_inner());
        (u.roots.iter().filter(|r| r.is_dir()).cloned().collect(), u.paused, u.registry_allowed, u.registry_baseline.clone())
    };
    if paused {
        // paused: no default folders, no drive search; the registry only for entries that are new since the launcher started the original installer
        if registry_allowed {
            for r in new_registry_dirs(registered_game_dirs(), &baseline) {
                if !roots.contains(&r) && !is_excluded(&r, &excluded) {
                    roots.push(r);
                }
            }
        }
        return roots;
    }
    for r in registered_game_dirs() {
        if !roots.contains(&r) {
            roots.push(r);
        }
    }
    roots.retain(|r| !is_excluded(r, &excluded));
    let mut add = |r: PathBuf| {
        if !roots.contains(&r) && !is_excluded(&r, &excluded) {
            roots.push(r);
        }
    };
    for r in game_roots(&real_env) {
        add(r);
    }
    // tests (and the preview script) switch the search of the real system off: only the folders named by the environment remain
    if real_env("TSW_LAUNCH_NO_SYSTEM_SEARCH").is_some() {
        return roots;
    }
    for letter in fixed_drive_letters() {
        for c in drive_candidates(letter) {
            if c.is_dir() {
                add(c);
            }
        }
    }
    roots
}

/// Folders in which the original installer puts the game by default (and where a Steam copy lives), most likely first. The list is short on purpose:
/// walking all of `Program Files` would be slow and could find unrelated programs. Anything else is chosen by the user.
pub fn game_roots(env: Env) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if !out.contains(&p) {
            out.push(p);
        }
    };
    let pf86 = env("ProgramFiles(x86)").map(PathBuf::from);
    let pf = env("ProgramFiles").map(PathBuf::from);
    let pf64 = env("ProgramW6432").map(PathBuf::from);
    for dir in [pf86.as_ref(), pf.as_ref(), pf64.as_ref()].into_iter().flatten() {
        push(dir.join("Funcom"));
    }
    for dir in [pf86.as_ref(), pf.as_ref(), pf64.as_ref()].into_iter().flatten() {
        push(dir.join("Steam").join("steamapps").join("common").join("The Secret World"));
    }
    out
}

/// The folder the installer is expected to create by default (for messages and for the first search).
pub fn default_game_dir(env: Env) -> Option<PathBuf> {
    env("ProgramFiles(x86)").or_else(|| env("ProgramFiles")).map(|d| PathBuf::from(d).join("Funcom").join("The Secret World"))
}

/// Windows folders that can never hold the launcher's data. Compared case-insensitively on the text of the path.
pub fn forbidden_base(base: &Path, env: Env) -> Option<&'static str> {
    let text = base.to_string_lossy().replace('/', "\\").to_ascii_lowercase();
    let text = text.trim_end_matches('\\');
    let under = |dir: Option<String>| {
        dir.map(|d| d.replace('/', "\\").to_ascii_lowercase()).map(|d| d.trim_end_matches('\\').to_string()).is_some_and(|d| !d.is_empty() && (text == d || text.starts_with(&format!("{d}\\"))))
    };
    if under(env("SystemRoot").or_else(|| env("windir"))) {
        return Some("the Windows folder cannot hold the launcher's data");
    }
    for var in ["ProgramFiles", "ProgramFiles(x86)", "ProgramW6432", "ProgramData"] {
        if under(env(var)) {
            return Some("choose a folder in your user profile or on a data drive, not Program Files or ProgramData (writing there needs administrator rights)");
        }
    }
    if text.starts_with("\\\\") {
        return Some("choose a local folder, not a network share");
    }
    None
}

// ---- starting programs --------------------------------------------------------------------------------------------------

/// Characters that cannot be handed to `cmd` safely inside quotes (`%` and `!` are expanded, `^ & | < >` and quotes end or change the command).
pub fn is_shell_safe(p: &Path) -> bool {
    let s = p.to_string_lossy();
    !s.is_empty() && !s.chars().any(|c| c.is_control() || matches!(c, '"' | '%' | '^' | '&' | '|' | '<' | '>' | '!'))
}

/// The argument line for `cmd.exe` that starts `program` in `dir` through ShellExecute (`start`): a program whose manifest asks for administrator rights
/// (the Funcom installer and patcher write below Program Files) cannot be started by `CreateProcess` (error 740), but Windows shows its UAC prompt for `start`.
/// `None` if a path contains characters that cannot be quoted safely; the caller then starts the program directly.
pub fn start_line(program: &Path, dir: &Path) -> Option<String> {
    if !is_shell_safe(program) || !is_shell_safe(dir) {
        return None;
    }
    // `/S` makes cmd remove exactly the outer pair of quotes, so the inner quoted paths stay intact
    Some(format!("/D /S /C \"start \"\" /D \"{}\" \"{}\"\"", dir.display(), program.display()))
}

/// Like [`start_line`], but hands Inno Setup (the original installers) the destination folder explicitly: `/DIR="<target>"`. Verified with an Inno Setup 6.7 probe:
/// `/DIR` has priority over the folder of an earlier installation of the same application, which would otherwise be reused without asking.
pub fn start_line_with_dir(program: &Path, dir: &Path, target: &Path) -> Option<String> {
    if !is_shell_safe(program) || !is_shell_safe(dir) || !is_shell_safe(target) || target.as_os_str().is_empty() {
        return None;
    }
    Some(format!("/D /S /C \"start \"\" /D \"{}\" \"{}\" /DIR=\"{}\"\"", dir.display(), program.display(), target.display()))
}

/// [`start_command`] with the destination folder for the installer. A target that cannot be quoted safely gives `None`: the caller must then not start the
/// installer at all (starting it without the folder would let it reuse an existing installation).
pub fn start_command_with_dir(program: &Path, dir: &Path, target: &Path) -> Option<Command> {
    let line = start_line_with_dir(program, dir, target)?;
    let mut c = Command::new("cmd");
    raw(&mut c, &line);
    no_window(&mut c);
    Some(c)
}

/// Make a child process of a GUI program start without its own console window (`CREATE_NO_WINDOW`). Without this a program that has no console itself
/// makes Windows open a new console window for every console child (`cmd`, `taskkill`, `tsw-launch.exe`). Not Windows: nothing to do.
#[cfg(windows)]
pub fn no_window(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000)
}

/// Not Windows: nothing to do.
#[cfg(not(windows))]
pub fn no_window(cmd: &mut Command) -> &mut Command {
    cmd
}

/// A command that starts `program` in `dir` without waiting for it. The returned `cmd` exits at once; the program is followed by name (see [`list_processes`]).
pub fn start_command(program: &Path, dir: &Path) -> Command {
    match start_line(program, dir) {
        Some(line) => {
            let mut c = Command::new("cmd");
            raw(&mut c, &line);
            no_window(&mut c);
            c
        }
        None => {
            let mut c = Command::new(program);
            c.current_dir(dir);
            no_window(&mut c);
            c
        }
    }
}

#[cfg(windows)]
fn raw(c: &mut Command, line: &str) {
    use std::os::windows::process::CommandExt;
    c.raw_arg(line);
}

#[cfg(not(windows))]
fn raw(c: &mut Command, line: &str) {
    c.arg(line); // never executed outside Windows; the shape is what the tests check
}

/// Ask a program's windows to close like the X button does: `taskkill` WITHOUT `/F` (a close request, never a forced end).
pub fn close_command(image_name: &str) -> Command {
    let mut c = Command::new("taskkill");
    c.args(["/IM", image_name]);
    no_window(&mut c);
    c
}

// ---- finding processes --------------------------------------------------------------------------------------------------

/// A running program found by name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WinProcess {
    pub pid: u32,
    /// Process name without `.exe`, as Windows reports it.
    pub name: String,
    pub path: Option<PathBuf>,
    pub started: Option<SystemTime>,
}

/// `100 ns` ticks since 1601-01-01 (a Windows FILETIME) as a point in time.
pub fn filetime_to_system_time(ticks: u64) -> Option<SystemTime> {
    const UNIX_EPOCH_AS_FILETIME: u64 = 116_444_736_000_000_000;
    ticks.checked_sub(UNIX_EPOCH_AS_FILETIME).map(|t| UNIX_EPOCH + Duration::from_nanos(t.saturating_mul(100)))
}

/// A program name for the process query: letters, digits and `.-_ ` only, `.exe` removed. Anything else is refused so no name can alter the script.
pub fn query_name(image: &str) -> Option<String> {
    let base = image.strip_suffix(".exe").or_else(|| image.strip_suffix(".EXE")).unwrap_or(image);
    (!base.is_empty() && base.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_' | ' '))).then(|| base.to_string())
}

/// The PowerShell script that prints one line `pid|name|path|filetime` per running process with one of `names` (access-denied fields stay empty).
pub fn process_script(names: &[&str]) -> Option<String> {
    let list: Option<Vec<String>> = names.iter().map(|n| query_name(n).map(|n| format!("'{n}'"))).collect();
    let list = list?;
    if list.is_empty() {
        return None;
    }
    Some(format!(
        "Get-Process -Name {} -ErrorAction SilentlyContinue | ForEach-Object {{ $t=''; try {{ $t=$_.StartTime.ToFileTimeUtc() }} catch {{}}; '{{0}}|{{1}}|{{2}}|{{3}}' -f $_.Id,$_.ProcessName,$_.Path,$t }}",
        list.join(",")
    ))
}

/// Parse the output of [`process_script`]. Lines that do not have the shape are ignored.
pub fn parse_process_lines(text: &str) -> Vec<WinProcess> {
    text.lines()
        .filter_map(|line| {
            let mut f = line.trim().splitn(4, '|');
            let pid = f.next()?.trim().parse::<u32>().ok()?;
            let name = f.next()?.trim().to_string();
            let path = f.next()?.trim();
            let ticks = f.next()?.trim();
            if name.is_empty() {
                return None;
            }
            Some(WinProcess { pid, name, path: (!path.is_empty()).then(|| PathBuf::from(path)), started: ticks.parse::<u64>().ok().and_then(filetime_to_system_time) })
        })
        .collect()
}

/// The programs among `found` whose name is one of `names` (case-insensitive, with or without `.exe`).
pub fn select_named(found: Vec<WinProcess>, names: &[&str]) -> Vec<WinProcess> {
    let wanted: Vec<String> = names.iter().filter_map(|n| query_name(n)).map(|n| n.to_ascii_lowercase()).collect();
    found.into_iter().filter(|p| wanted.contains(&p.name.to_ascii_lowercase())).collect()
}

/// Run the process query. Empty on any failure (PowerShell missing, blocked, timeout): "nothing is running" is the safe answer for a watcher that only
/// ever REACTS to what it sees, never for something that is destroyed on a guess.
#[cfg(windows)]
pub fn list_processes(names: &[&str]) -> Vec<WinProcess> {
    let Some(script) = process_script(names) else { return Vec::new() };
    let mut cmd = Command::new("powershell");
    cmd.args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script]).stdin(std::process::Stdio::null());
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW: no console flash
    match cmd.output() {
        Ok(out) if out.status.success() => select_named(parse_process_lines(&String::from_utf8_lossy(&out.stdout)), names),
        _ => Vec::new(),
    }
}

/// Outside Windows there is nothing to list.
#[cfg(not(windows))]
pub fn list_processes(_names: &[&str]) -> Vec<WinProcess> {
    Vec::new()
}

// ---- file dialogs ---------------------------------------------------------------------------------------------------------

/// A dialog title for the PowerShell script: letters, digits, spaces and `.,:()-_` only, everything else becomes a space (so no title can end the string).
pub fn dialog_title(title: &str) -> String {
    title.chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '.' | ',' | ':' | '(' | ')' | '-' | '_') { c } else { ' ' }).take(120).collect()
}

/// The PowerShell script that shows Windows' own file or folder dialog and prints the chosen path (UTF-8, nothing when cancelled).
pub fn pick_script(title: &str, directory: bool) -> String {
    let title = dialog_title(title);
    let head = "Add-Type -AssemblyName System.Windows.Forms; [System.Windows.Forms.Application]::EnableVisualStyles(); [Console]::OutputEncoding=[System.Text.Encoding]::UTF8; $o=New-Object System.Windows.Forms.Form; $o.TopMost=$true;";
    if directory {
        format!("{head} $d=New-Object System.Windows.Forms.FolderBrowserDialog; $d.Description='{title}'; if ($d.ShowDialog($o) -eq [System.Windows.Forms.DialogResult]::OK) {{ [Console]::Out.Write($d.SelectedPath) }}")
    } else {
        format!(
            "{head} $d=New-Object System.Windows.Forms.OpenFileDialog; $d.Title='{title}'; $d.Filter='Programs (*.exe)|*.exe|All files (*.*)|*.*'; if ($d.ShowDialog($o) -eq [System.Windows.Forms.DialogResult]::OK) {{ [Console]::Out.Write($d.FileName) }}"
        )
    }
}

/// A Windows path in text form: a drive letter and a backslash or slash, or a `\\server\share` path. Used on the dialog output (the check does not depend on the
/// platform the code runs on, so it is tested everywhere).
pub fn is_windows_absolute(text: &str) -> bool {
    let b = text.as_bytes();
    (b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && (b[2] == b'\\' || b[2] == b'/')) || text.starts_with("\\\\")
}

/// The path printed by the dialog script. Empty output (cancelled), control characters or a relative path give `None`.
pub fn parse_pick_output(raw: &[u8]) -> Option<PathBuf> {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_start_matches('\u{feff}').trim();
    (!text.is_empty() && !text.chars().any(char::is_control) && is_windows_absolute(text)).then(|| PathBuf::from(text))
}

/// Show the native dialog. `Ok(None)` = cancelled. Blocks until the dialog is closed: call it from a worker thread.
#[cfg(windows)]
pub fn pick(title: &str, directory: bool) -> Result<Option<PathBuf>, String> {
    use std::os::windows::process::CommandExt;
    let out = Command::new("powershell")
        .args(["-NoLogo", "-NoProfile", "-NonInteractive", "-STA", "-Command", &pick_script(title, directory)])
        .stdin(std::process::Stdio::null())
        .creation_flags(0x0800_0000) // CREATE_NO_WINDOW: only the dialog is shown
        .output()
        .map_err(|e| format!("the file dialog could not be started: {e}"))?;
    if !out.status.success() {
        return Err(format!("the file dialog failed ({})", out.status));
    }
    Ok(parse_pick_output(&out.stdout))
}

/// There is no native dialog outside Windows (Linux uses the desktop portal).
#[cfg(not(windows))]
pub fn pick(_title: &str, _directory: bool) -> Result<Option<PathBuf>, String> {
    Err("no native file dialog on this platform".into())
}

// ---- the Direct3D runtime -----------------------------------------------------------------------------------------------

/// The Direct3D 9/10/11 helper libraries the original client loads (DirectX June 2010 runtime). Windows 10/11 does not ship them by default.
pub const DIRECTX_LIBRARIES: [&str; 3] = ["d3dcompiler_43.dll", "d3dx9_43.dll", "d3dx11_43.dll"];

/// Which of [`DIRECTX_LIBRARIES`] are missing from the given system folders (System32 and SysWOW64; the 32-bit client needs the SysWOW64 copy on
/// 64-bit Windows). Pure over a "file exists" predicate.
pub fn directx_missing(dirs: &[PathBuf], exists: &dyn Fn(&Path) -> bool) -> Vec<&'static str> {
    DIRECTX_LIBRARIES.iter().copied().filter(|lib| !dirs.iter().any(|d| exists(&d.join(lib)))).collect()
}

/// The folders searched by [`directx_missing`]: the 32-bit system folder first (`SysWOW64` on 64-bit Windows), then `System32`.
pub fn system_dirs(env: Env) -> Vec<PathBuf> {
    let Some(root) = env("SystemRoot").or_else(|| env("windir")) else { return Vec::new() };
    let root = PathBuf::from(root);
    vec![root.join("SysWOW64"), root.join("System32")]
}

/// What the launcher tells the user when libraries are missing. It does NOT download or install anything: the End-User Runtime is Microsoft's package and
/// its licence terms are accepted by the user in Microsoft's own installer.
pub fn directx_runtime(missing: &[&str]) -> Option<String> {
    (!missing.is_empty()).then(|| {
        format!(
            "These Direct3D libraries are missing on this computer: {}. The original client may fail to start without them. They belong to Microsoft's \"DirectX End-User Runtimes (June 2010)\"; install that package from Microsoft's website yourself (the launcher does not download Microsoft software on Windows) or let the original Funcom installer set it up.",
            missing.join(", ")
        )
    })
}

/// The tests that change the process-wide search state run one after the other.
#[cfg(test)]
pub(crate) static SEARCH_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn env_of(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn data_lives_below_localappdata_in_the_user_profile() {
        let e = env_of(&[("LOCALAPPDATA", "C:\\Users\\Anna\\AppData\\Local")]);
        assert_eq!(data_dir(&|k| e.get(k).cloned()), Some(PathBuf::from("C:\\Users\\Anna\\AppData\\Local").join("tsw-forever")));
        assert_eq!(data_dir(&|_| None), None);
    }

    #[test]
    fn settings_live_below_the_roaming_folder_from_windows_with_the_variable_as_fallback_only() {
        let e = env_of(&[("APPDATA", "C:\\Users\\Env\\AppData\\Roaming")]);
        let env = |k: &str| e.get(k).cloned();
        let known = Some(PathBuf::from("C:\\Users\\Anna\\AppData\\Roaming"));
        assert_eq!(config_dir(known.clone(), &env), Some(PathBuf::from("C:\\Users\\Anna\\AppData\\Roaming").join("tsw-forever")), "Windows' answer wins over the variable");
        assert_eq!(config_file(known, &env), Some(PathBuf::from("C:\\Users\\Anna\\AppData\\Roaming").join("tsw-forever").join("config")));
        assert_eq!(config_dir(None, &env), Some(PathBuf::from("C:\\Users\\Env\\AppData\\Roaming").join("tsw-forever")));
        assert_eq!(config_dir(None, &|_| None), None);
    }

    #[cfg(windows)]
    #[test]
    fn windows_names_an_absolute_roaming_folder() {
        let p = roaming_appdata().expect("the Known Folder API answers on Windows");
        assert!(p.is_absolute() && p.is_dir(), "{p:?}");
    }

    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn the_folder_the_funcom_installer_recorded_is_read_back_exactly() {
        let out = format!(
            "{}\n\u{feff}{}\r\n{}\n{}\nnot hex\n{}\n\n{}\n",
            hex("C:\\Games\\The Secret World"),
            hex("C:\\Games\\The Secret World\\"),
            hex("D:\\Spiele Übung\\TSW"),
            hex("relative\\path"),
            "abc",
            hex("c:\\games\\the secret world")
        );
        assert_eq!(
            parse_registered_dirs(&out),
            vec![PathBuf::from("C:\\Games\\The Secret World"), PathBuf::from("D:\\Spiele Übung\\TSW")],
            "trailing separator removed, case-insensitive duplicates and garbage dropped, Unicode kept"
        );
        assert!(parse_registered_dirs("").is_empty() && parse_registered_dirs("zz\n1\n").is_empty());
    }

    #[test]
    fn excluded_folders_match_by_name_case_insensitively_and_only_as_whole_folders() {
        let ex = vec![PathBuf::from("C:\\Games\\The Secret World")];
        for hit in ["C:\\Games\\The Secret World", "c:\\games\\the secret world\\", "C:/Games/The Secret World/RDB/x.bin", "C:\\GAMES\\THE SECRET WORLD\\TheSecretWorldDX11.exe"] {
            assert!(is_excluded(Path::new(hit), &ex), "{hit}");
        }
        for miss in ["C:\\Games\\The Secret World 2", "C:\\Games", "D:\\Games\\The Secret World", "C:\\Games\\The Secret Worlds\\x", "C:\\TSW-Test"] {
            assert!(!is_excluded(Path::new(miss), &ex), "{miss}");
        }
        assert!(!is_excluded(Path::new("C:\\anything"), &[]), "no test mode, no exclusion");
        let e = env_of(&[(ENV_EXCLUDE_DIRS, " C:\\a ; ;D:\\b\\c ")]);
        assert_eq!(excluded_dirs(&|k| e.get(k).cloned()), vec![PathBuf::from("C:\\a"), PathBuf::from("D:\\b\\c")]);
        assert!(excluded_dirs(&|_| None).is_empty());
    }

    #[test]
    fn only_registry_entries_that_are_new_since_the_installer_started_count() {
        let baseline = vec![PathBuf::from("C:\\Games\\The Secret World")];
        let found = vec![PathBuf::from("c:/games/the secret world/"), PathBuf::from("D:\\Neu\\The Secret World"), PathBuf::from("C:\\Games\\The Secret World\\Sub")];
        assert_eq!(new_registry_dirs(found, &baseline), vec![PathBuf::from("D:\\Neu\\The Secret World")], "the old folder, in any spelling or inside it, is not new");
        assert_eq!(new_registry_dirs(vec![PathBuf::from("E:\\X")], &[]), vec![PathBuf::from("E:\\X")], "without a baseline every entry is new");
        assert!(new_registry_dirs(Vec::new(), &baseline).is_empty());
    }

    #[cfg(windows)]
    #[test]
    fn a_paused_detection_finds_nothing_until_the_installer_was_started() {
        let _serial = SEARCH_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::set_var(ENV_TEST_DIRECTX_MISSING, "none"); // harmless; keeps the process environment test-owned
        std::env::remove_var(ENV_TEST_DIRECTX_MISSING);
        set_autodetect(true, false, Vec::new(), false);
        assert!(autodetect_paused() && !stored_paths_honored());
        assert!(search_roots().is_empty(), "no default folder, no drive search, no registry while paused");
        set_autodetect(true, false, Vec::new(), true);
        assert!(stored_paths_honored(), "a deliberate choice makes stored paths count");
        reset_user_search();
        assert!(!autodetect_paused() && stored_paths_honored(), "neutral state");
    }

    #[test]
    fn the_installer_gets_its_destination_folder_explicitly_and_only_if_it_can_be_quoted_safely() {
        let line = start_line_with_dir(Path::new("C:\\Users\\A\\dl\\TheSecretWorldInstaller.exe"), Path::new("C:\\Users\\A\\dl"), Path::new("D:\\Spiele neu\\The Secret World")).unwrap();
        assert!(line.ends_with("\"C:\\Users\\A\\dl\\TheSecretWorldInstaller.exe\" /DIR=\"D:\\Spiele neu\\The Secret World\"\""), "{line}");
        assert!(line.starts_with("/D /S /C \"start \"\" /D "));
        for bad in ["D:\\a&b", "D:\\100%", "D:\\a^b", "D:\\hi!", "D:\\a\"b", ""] {
            assert!(start_line_with_dir(Path::new("C:\\x.exe"), Path::new("C:\\"), Path::new(bad)).is_none(), "{bad:?}");
            assert!(start_command_with_dir(Path::new("C:\\x.exe"), Path::new("C:\\"), Path::new(bad)).is_none(), "{bad:?}");
        }
    }

    /// A folder that does not exist yet, named through the 8.3 short name of an existing parent (the CI runner's temp folder is `RUNNER~1`), still lies inside it.
    #[cfg(windows)]
    #[test]
    fn a_missing_folder_below_an_existing_one_overlaps_it_also_through_a_short_name() {
        let _exec = crate::test_exec_lock(); // starts PowerShell
        let base = std::env::temp_dir().join(format!("tsw-shortname-{}", std::process::id())).join("Ein langer Ordnername fuer den Test");
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
        std::fs::create_dir_all(&base).unwrap();
        let script = format!("(New-Object -ComObject Scripting.FileSystemObject).GetFolder('{}').ShortPath", base.display());
        let out = Command::new("powershell").args(["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", &script]).output().unwrap();
        let short = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim());
        if short.as_os_str().is_empty() || short == base {
            let _ = std::fs::remove_dir_all(base.parent().unwrap());
            return; // this machine hands out no short names: nothing to check
        }
        assert!(overlaps(&short.join("noch").join("nicht da"), &base), "{short:?} vs {base:?}");
        assert!(overlaps(&base, &short.join("noch")), "and the other way round");
        let _ = std::fs::remove_dir_all(base.parent().unwrap());
    }

    #[test]
    fn folders_overlap_when_one_is_the_other_or_inside_it() {
        assert!(overlaps(Path::new("C:\\Games\\The Secret World"), Path::new("c:/games/the secret world/")));
        assert!(overlaps(Path::new("C:\\Games"), Path::new("C:\\Games\\The Secret World")));
        assert!(overlaps(Path::new("C:\\Games\\The Secret World\\RDB"), Path::new("C:\\Games\\The Secret World")));
        assert!(!overlaps(Path::new("C:\\Games\\The Secret World 2"), Path::new("C:\\Games\\The Secret World")));
        assert!(!overlaps(Path::new("D:\\TSW"), Path::new("C:\\Games\\The Secret World")));
    }

    #[test]
    fn missing_directx_libraries_can_be_simulated_for_a_test_only() {
        // the real check is the pure function; the simulation names libraries or `all`
        let all: Vec<&str> = DIRECTX_LIBRARIES.to_vec();
        let pick = |v: &str| DIRECTX_LIBRARIES.iter().copied().filter(|l| v.split(',').any(|x| x.trim().eq_ignore_ascii_case("all") || x.trim().eq_ignore_ascii_case(l))).collect::<Vec<_>>();
        assert_eq!(pick("all"), all);
        assert_eq!(pick("D3DX9_43.dll, d3dx11_43.dll"), vec!["d3dx9_43.dll", "d3dx11_43.dll"]);
        assert!(pick("other.dll").is_empty());
    }

    #[test]
    fn self_chosen_folders_on_a_drive_are_a_short_named_list() {
        let c = drive_candidates('d');
        assert!(c.contains(&PathBuf::from("D:\\Games\\The Secret World")) && c.contains(&PathBuf::from("D:\\Funcom\\The Secret World")));
        assert!(c.len() <= 8 && c.iter().all(|p| p.to_string_lossy().starts_with("D:\\")), "only named folders, never the whole drive: {c:?}");
    }

    #[cfg(windows)]
    #[test]
    fn the_fixed_drives_of_this_computer_are_found() {
        let d = fixed_drive_letters();
        assert!(d.contains(&'C'), "{d:?}");
    }

    #[test]
    fn the_registry_script_reads_only_the_games_uninstall_key() {
        let s = registry_script();
        assert!(s.contains("The Secret World_is1") && s.contains("Inno Setup: App Path") && s.contains("OpenSubKey"), "{s}");
        assert!(!s.contains("SetValue") && !s.contains("Remove") && !s.contains("New-Item") && !s.contains("CreateSubKey"), "the script only reads");
    }

    #[test]
    fn migration_copies_old_settings_and_never_overwrites_or_deletes() {
        let base = std::env::temp_dir().join(format!("tsw-migrate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (old, new) = (base.join("tsw-launch"), base.join("tsw-forever"));
        std::fs::create_dir_all(old.join("sub")).unwrap();
        std::fs::write(old.join("config"), "client_exe = C:\\Spiele\\TSW\\TheSecretWorldDX11.exe\n").unwrap();
        std::fs::write(old.join("install-state"), "old state").unwrap();
        std::fs::write(old.join("sub").join("x"), "not followed").unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("install-state"), "newer state").unwrap();
        let copied = migrate_legacy_settings(&new, &old).unwrap();
        assert_eq!(copied, vec![new.join("config")]);
        assert_eq!(std::fs::read_to_string(new.join("config")).unwrap(), "client_exe = C:\\Spiele\\TSW\\TheSecretWorldDX11.exe\n");
        assert_eq!(std::fs::read_to_string(new.join("install-state")).unwrap(), "newer state", "an existing file is never overwritten");
        assert!(!new.join("sub").exists(), "sub-folders are not copied");
        assert!(old.join("config").is_file() && old.join("install-state").is_file(), "the old folder stays complete");
        assert!(migrate_legacy_settings(&new, &old).unwrap().is_empty(), "a second run changes nothing");
        assert!(migrate_legacy_settings(&new, &base.join("missing")).unwrap().is_empty());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn the_default_game_locations_are_a_short_list_without_duplicates() {
        let e = env_of(&[("ProgramFiles(x86)", "C:\\Program Files (x86)"), ("ProgramFiles", "C:\\Program Files"), ("ProgramW6432", "C:\\Program Files")]);
        let roots = game_roots(&|k| e.get(k).cloned());
        assert_eq!(roots[0], PathBuf::from("C:\\Program Files (x86)").join("Funcom"));
        assert_eq!(roots.iter().filter(|r| **r == PathBuf::from("C:\\Program Files").join("Funcom")).count(), 1, "ProgramW6432 equals ProgramFiles: listed once");
        assert!(roots.iter().any(|r| r.ends_with("The Secret World") && r.to_string_lossy().contains("steamapps")));
        assert!(roots.len() <= 6);
        assert_eq!(default_game_dir(&|k| e.get(k).cloned()), Some(PathBuf::from("C:\\Program Files (x86)").join("Funcom").join("The Secret World")));
        assert!(game_roots(&|_| None).is_empty());
    }

    #[test]
    fn program_files_programdata_the_windows_folder_and_shares_cannot_hold_the_data() {
        let e = env_of(&[("SystemRoot", "C:\\Windows"), ("ProgramFiles", "C:\\Program Files"), ("ProgramFiles(x86)", "C:\\Program Files (x86)"), ("ProgramData", "C:\\ProgramData")]);
        let env = |k: &str| e.get(k).cloned();
        for bad in ["C:\\Windows\\Temp\\x", "c:\\windows", "C:\\Program Files\\TSW", "C:/Program Files (x86)/TSW-Forever", "C:\\ProgramData\\tsw", "\\\\server\\share\\tsw"] {
            assert!(forbidden_base(Path::new(bad), &env).is_some(), "{bad}");
        }
        for ok in ["C:\\Users\\Anna\\AppData\\Local\\TSW-Forever", "D:\\Games\\TSW-Forever", "C:\\Program Filesystem\\x", "C:\\WindowsGames\\x"] {
            assert!(forbidden_base(Path::new(ok), &env).is_none(), "{ok}");
        }
    }

    #[test]
    fn the_start_line_quotes_paths_with_spaces_and_hands_elevation_to_windows() {
        let line = start_line(Path::new("C:\\Program Files (x86)\\Funcom\\The Secret World\\ClientPatcher.exe"), Path::new("C:\\Program Files (x86)\\Funcom\\The Secret World")).unwrap();
        assert_eq!(line, "/D /S /C \"start \"\" /D \"C:\\Program Files (x86)\\Funcom\\The Secret World\" \"C:\\Program Files (x86)\\Funcom\\The Secret World\\ClientPatcher.exe\"\"");
        assert!(line.starts_with("/D /S /C \"start \"\" /D "), "an empty window title comes first, or start would take the quoted path as the title");
    }

    #[test]
    fn paths_that_cmd_could_misread_are_never_put_into_a_command_line() {
        for bad in ["C:\\a&b\\x.exe", "C:\\100%\\x.exe", "C:\\a^b\\x.exe", "C:\\a|b\\x.exe", "C:\\a<b\\x.exe", "C:\\a\"b\\x.exe", "C:\\hi!\\x.exe", "C:\\a\nb\\x.exe", ""] {
            assert!(start_line(Path::new(bad), Path::new("C:\\ok")).is_none(), "{bad:?}");
            assert!(start_line(Path::new("C:\\ok\\x.exe"), Path::new(bad)).is_none(), "{bad:?}");
        }
        // the caller then starts the program directly, in its own folder
        let c = start_command(Path::new("C:\\a&b\\x.exe"), Path::new("C:\\a&b"));
        assert_eq!(c.get_program(), "C:\\a&b\\x.exe");
        assert_eq!(c.get_current_dir(), Some(Path::new("C:\\a&b")));
        assert_eq!(start_command(Path::new("C:\\ok\\x.exe"), Path::new("C:\\ok")).get_program(), "cmd");
    }

    #[test]
    fn a_program_is_asked_to_close_never_forced() {
        let c = close_command("ClientPatcher.exe");
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(c.get_program(), "taskkill");
        assert_eq!(args, ["/IM", "ClientPatcher.exe"], "no /F, no /PID, no wildcard");
    }

    #[test]
    fn filetime_is_converted_to_unix_time() {
        assert_eq!(filetime_to_system_time(116_444_736_000_000_000), Some(UNIX_EPOCH));
        assert_eq!(filetime_to_system_time(116_444_736_010_000_000), Some(UNIX_EPOCH + Duration::from_secs(1)));
        assert_eq!(filetime_to_system_time(0), None, "before 1970 is not a plausible start time");
    }

    #[test]
    fn only_plain_program_names_reach_the_powershell_script() {
        assert_eq!(query_name("ClientPatcher.exe").as_deref(), Some("ClientPatcher"));
        assert_eq!(query_name("TheSecretWorldInstaller.tmp").as_deref(), Some("TheSecretWorldInstaller.tmp"));
        for bad in ["a';calc;'", "x y$z", "a|b", "", ".exe", "a\"b", "a`b", "a;b", "a(b)"] {
            assert!(query_name(bad).is_none(), "{bad:?}");
        }
        let script = process_script(&["ClientPatcher.exe", "TheSecretWorldInstaller.exe"]).unwrap();
        assert!(script.starts_with("Get-Process -Name 'ClientPatcher','TheSecretWorldInstaller' -ErrorAction SilentlyContinue"));
        assert!(!script.contains("Invoke-Expression") && !script.contains("iex"));
        assert!(process_script(&["bad;name"]).is_none());
        assert!(process_script(&[]).is_none());
    }

    #[test]
    fn the_process_listing_is_parsed_and_unusable_lines_are_ignored() {
        let text = "1204|ClientPatcher|C:\\Program Files (x86)\\Funcom\\The Secret World\\ClientPatcher.exe|133800000000000000\r\n77|TheSecretWorldInstaller||\r\nnoise\r\n|x||\r\nabc|y|z|1\r\n";
        let p = parse_process_lines(text);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].pid, 1204);
        assert_eq!(p[0].name, "ClientPatcher");
        assert!(p[0].path.as_ref().unwrap().to_string_lossy().ends_with("ClientPatcher.exe"));
        assert!(p[0].started.is_some());
        assert_eq!((p[1].pid, p[1].path.clone(), p[1].started), (77, None, None), "access-denied fields stay empty and the process still counts");
        let found = select_named(p, &["clientpatcher.EXE"]);
        assert_eq!(found.len(), 1, "names are compared case-insensitively, with or without .exe");
    }

    #[test]
    fn missing_direct3d_libraries_are_named_and_nothing_is_downloaded() {
        let dirs = vec![PathBuf::from("C:\\Windows").join("SysWOW64"), PathBuf::from("C:\\Windows").join("System32")];
        let have = |p: &Path| (p.to_string_lossy().contains("SysWOW64") && p.ends_with("d3dx9_43.dll")) || (p.to_string_lossy().contains("System32") && p.ends_with("d3dcompiler_43.dll"));
        assert_eq!(directx_missing(&dirs, &have), vec!["d3dx11_43.dll"]);
        assert!(directx_missing(&dirs, &|_| true).is_empty());
        assert_eq!(directx_runtime(&[]), None);
        let msg = directx_runtime(&["d3dx11_43.dll"]).unwrap();
        assert!(msg.contains("d3dx11_43.dll") && msg.contains("does not download Microsoft software"));
        let e = env_of(&[("SystemRoot", "C:\\Windows")]);
        assert_eq!(system_dirs(&|k| e.get(k).cloned()), dirs);
        assert!(system_dirs(&|_| None).is_empty());
    }

    #[test]
    fn windows_offers_no_wine_features_and_linux_keeps_all_of_them() {
        let w = Capabilities::WINDOWS;
        assert!(!w.wine_runtime && !w.gamescope && !w.fps_limit && !w.wine_desktop && !w.microsoft_components && !w.flatpak && !w.linux_doctor);
        assert!(w.resolution && w.display_mode);
        let l = Capabilities::LINUX;
        assert!(l.wine_runtime && l.gamescope && l.fps_limit && l.wine_desktop && l.microsoft_components && l.flatpak && l.resolution && l.display_mode && l.linux_doctor);
        assert_eq!(Capabilities::current(), if cfg!(windows) { w } else { l });
    }
    #[test]
    fn the_dialog_script_cannot_be_altered_by_the_title_and_the_output_is_checked() {
        assert_eq!(dialog_title("The Secret World installer"), "The Secret World installer");
        let evil = pick_script("x'; Remove-Item C:\\ -Recurse; '", false);
        assert!(!evil.contains("Remove-Item C:\\ -Recurse; '"), "{evil}");
        assert_eq!(evil.matches('\'').count(), 4, "only the two quoted strings of the script remain: {evil}");
        assert!(pick_script("Folder", true).contains("FolderBrowserDialog") && pick_script("File", false).contains("OpenFileDialog"));
        assert_eq!(parse_pick_output(b"\xEF\xBB\xBFC:\\Games\\TSW\\TheSecretWorldDX11.exe\r\n"), Some(PathBuf::from("C:\\Games\\TSW\\TheSecretWorldDX11.exe")));
        assert_eq!(parse_pick_output(b"\\\\nas\\games\\tsw"), Some(PathBuf::from("\\\\nas\\games\\tsw")));
        for bad in [&b""[..], b"  \r\n", b"relative\\path", b"C:\\a\x07b", b"..\\x"] {
            assert!(parse_pick_output(bad).is_none(), "{bad:?}");
        }
        assert!(is_windows_absolute("D:/Games") && !is_windows_absolute("/usr/bin") && !is_windows_absolute("C:"));
    }
}
