//! Installation manager: sets up a new TSW installation (folders, installer download or local installer, Wine/Proton prefix,
//! running the original installer) or adopts an existing one, inside ONE user-chosen base folder (default `~/Games/TheSecretWorld`):
//!
//! ```text
//! <base>/prefix/     Wine/Proton prefix created by the launcher; with the installer's default Windows path the game lives INSIDE it
//!                    (`prefix/drive_c/Program Files (x86)/Funcom/The Secret World/`), so there is no second copy of the game data
//! <base>/downloads/  installer cache (the .part file lives here while a download is incomplete)
//! <base>/patches/    work folder of the client patch system (downloads, staging, backups, journal)
//! <base>/game/       only for an explicit import/adopt mode; never required and not created by default
//! ```
//!
//! Safety rules, each covered by tests:
//! * the launcher itself writes nothing outside `<base>` (its own settings live in the XDG config directory, handled by the caller); the original
//!   installer runs under Wine's normal drive mappings (Z: = /, user folders) and is NOT confined by the launcher;
//! * an existing, non-empty `prefix/` that the launcher did not create is never written into, replaced or deleted; an existing installation is adopted;
//! * the only download source is the allow-listed official host ([`TRUSTED_HOST`]), the user cannot type an arbitrary URL;
//! * a download starts only with explicit consent AND a pinned SHA-256 (shipped in [`OFFICIAL`] or pinned by the user after
//!   checking it out of band); the file is renamed to its final name only after the checksum matched;
//! * a local installer file is copied into the cache and identified by its SHA-256; an unknown file is never trusted silently: running it needs
//!   a confirmation that is bound to exactly that fingerprint ([`State::confirmed_sha256`]);
//! * the installation counts as complete only when the game files are really there ([`find_installation`]); the path is detected, never assumed;
//! * no `sudo`, no shell: every external program (`curl`, Wine) is started with an argument vector.
//!
//! The state is derived from the file system ([`assess`]); the small state file only keeps the user's decisions.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::progress::{Outcome, Phase, Progress, RateMeter, Sink, Update};
use crate::sha256;

/// The only host the launcher downloads the installer from.
pub const TRUSTED_HOST: &str = "cdn.funcom.com";
pub const INSTALLER_NAME: &str = "TheSecretWorldInstaller.exe";
pub const CLIENT_EXE: &str = crate::discover::CLIENT_EXE;
/// Shown before the original installer is started (CLI and GUI). The German text is in the GUI translation.
pub const INSTALLER_NOTICE: &str = "Please use the suggested default settings while installing The Secret World and do not change the installation path in particular. The suggested Windows path is inside the Wine/Proton environment managed by our launcher.";
pub const PATCHER_EXE: &str = "ClientPatcher.exe";

/// A download source: where, and the checksum it must have (if one is known).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Source {
    pub url: &'static str,
    /// Lower-case hex SHA-256 of the file. `None` = no trusted checksum is known, so the download stays blocked.
    pub sha256: Option<&'static str>,
    /// Where this entry comes from (for the documentation and the user).
    pub origin: &'static str,
}

/// The official Funcom installer. The address comes from the Lutris installer script that the project owner uses
/// (`the-secret-world-classic-online-installer`, the original component measurements); the server answers identically over HTTPS
/// (valid certificate), so HTTPS is used and plain HTTP is refused.
/// **The pinned checksum is a trust-on-first-use measurement, not a value published by Funcom**: SHA-256 of the file downloaded over HTTPS on
/// 2026-10-31 (33 164 664 bytes, `Last-Modified` 22 Jun 2017). The file carries an Authenticode signature (Funcom Oslo, Symantec/VeriSign
/// chain) whose validity was NOT verified here. The pin protects against a changed or damaged download, not against a file that was already
/// wrong when measured. See the original component measurements. Whoever prefers an independent check supplies their
/// own installer (checksum shown before it is run) or pins another value.
pub const OFFICIAL: Source = Source {
    url: "https://cdn.funcom.com/downloads/tsw/client/TheSecretWorldInstaller.exe",
    sha256: Some("9b9a71fbf198d5eab7fbea80a1cc93994d79d1526a1e199f62532fc2705d908e"),
    origin: "trust-on-first-use measurement over HTTPS, 2026-10-31 (the original component measurements)",
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallError {
    /// The base folder is not acceptable (message names the rule).
    BadBase(String),
    /// A folder the manager would create contents in is not empty / not ours.
    NotEmpty(PathBuf),
    /// The download is not allowed (no consent, no checksum, untrusted host).
    Blocked(String),
    Cancelled,
    Failed(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::BadBase(m) | InstallError::Blocked(m) | InstallError::Failed(m) => write!(f, "{m}"),
            InstallError::NotEmpty(p) => write!(f, "{} already contains files; the launcher never writes into or replaces existing data (adopt it instead)", p.display()),
            InstallError::Cancelled => write!(f, "cancelled; the partial download is kept and resumed next time"),
        }
    }
}

impl std::error::Error for InstallError {}

fn failed(what: &str, e: io::Error) -> InstallError {
    InstallError::Failed(format!("{what}: {e}"))
}

// ---- layout ---------------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Layout {
    pub base: PathBuf,
}

/// System directories that are never an installation base (or inside one).
const FORBIDDEN_ROOTS: &[&str] = &["/bin", "/boot", "/dev", "/etc", "/lib", "/lib32", "/lib64", "/proc", "/root", "/run", "/sbin", "/sys", "/usr", "/var"];

impl Layout {
    /// The Wine prefix; the game lives INSIDE it ([`game_path`]), there is no separate game folder and no link.
    pub fn prefix(&self) -> PathBuf {
        self.base.join("prefix")
    }
    /// Wine/Proton runtimes the launcher provides (GE-Proton).
    pub fn runtime(&self) -> PathBuf {
        self.base.join("runtime")
    }
    pub fn downloads(&self) -> PathBuf {
        self.base.join("downloads")
    }
    /// Persistent launcher caches (verified shader DLLs, patch work folder); everything in it can be fetched again.
    pub fn cache(&self) -> PathBuf {
        self.base.join("cache")
    }
    pub fn native_cache(&self) -> PathBuf {
        self.cache().join("native")
    }
    pub fn patches(&self) -> PathBuf {
        self.cache().join("patches")
    }
    pub fn logs(&self) -> PathBuf {
        self.base.join("logs")
    }
    pub fn installer(&self) -> PathBuf {
        self.downloads().join(INSTALLER_NAME)
    }
    pub fn partial(&self) -> PathBuf {
        self.downloads().join(format!("{INSTALLER_NAME}.part"))
    }

    /// Validate a user-chosen base folder. Pure path rules (no file access): absolute, no `..`/`.` tricks, not a system
    /// directory, not the home directory or one of its parents, and deep enough not to be a mount point or `/home`.
    pub fn new(base: &Path, home: Option<&Path>) -> Result<Layout, InstallError> {
        let bad = |m: &str| Err(InstallError::BadBase(format!("{m}: {}", base.display())));
        if !base.is_absolute() {
            return bad("the installation folder must be an absolute path");
        }
        // `Path::components` silently drops a middle `.`, so the raw text is checked as well
        if base.components().any(|c| matches!(c, Component::ParentDir | Component::CurDir)) || base.to_string_lossy().split('/').any(|c| c == "." || c == "..") {
            return bad("the installation folder may not contain '.' or '..'");
        }
        let depth = base.components().filter(|c| matches!(c, Component::Normal(_))).count();
        if depth < 2 {
            return bad("choose a folder inside your home or on a data drive, not a top-level folder");
        }
        if FORBIDDEN_ROOTS.iter().any(|r| base.starts_with(r)) {
            return bad("system folders cannot hold the installation");
        }
        if let Some(h) = home {
            if h.starts_with(base) {
                return bad("the installation folder may not be your home folder or one of its parents");
            }
        }
        if base.to_string_lossy().chars().any(|c| c.is_control() || c == '\n') {
            return bad("the installation folder contains control characters");
        }
        Ok(Layout { base: base.to_path_buf() })
    }
}

fn is_symlink(p: &Path) -> bool {
    std::fs::symlink_metadata(p).map(|m| m.file_type().is_symlink()).unwrap_or(false)
}

/// Find the client executable below `dir` (at most 5 levels, no symlink is followed).
pub fn find_client(dir: &Path) -> Option<PathBuf> {
    find_file(dir, CLIENT_EXE)
}

/// Find a file by its (case-insensitive) name below `dir`, at most 5 levels deep, following no symlink.
pub fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: u32, name: &str) -> Option<PathBuf> {
        let mut subdirs = Vec::new();
        for e in std::fs::read_dir(dir).ok()?.flatten() {
            let ft = e.file_type().ok()?;
            if ft.is_file() && e.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
                return Some(e.path());
            }
            if ft.is_dir() && depth < 5 {
                subdirs.push(e.path());
            }
        }
        subdirs.sort();
        subdirs.into_iter().find_map(|d| walk(&d, depth + 1, name))
    }
    walk(dir, 0, name)
}

/// Does `dir/<parts...>` exist, comparing names case-insensitively (the installer decides the case, not us)?
fn exists_ci(dir: &Path, parts: &[&str]) -> bool {
    let Some((first, rest)) = parts.split_first() else {
        return true;
    };
    let Ok(rd) = std::fs::read_dir(dir) else {
        return false;
    };
    rd.flatten().any(|e| e.file_name().to_string_lossy().eq_ignore_ascii_case(first) && exists_ci(&e.path(), rest))
}

/// A game installation found on disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found {
    pub client: PathBuf,
    /// The client's data is there too (`RDB/RDBHashIndex.bin` next to it).
    pub complete: bool,
}

/// Where the original installer puts the game by default: below the prefix.
pub fn install_roots(layout: &Layout) -> Vec<PathBuf> {
    let c = layout.prefix().join("drive_c");
    vec![game_path(layout), c.join("Program Files (x86)"), c.join("Program Files")]
}

/// Where the original installer puts the game, directly inside the prefix (`C:\Program Files (x86)\Funcom\The Secret World`).
pub fn game_path(layout: &Layout) -> PathBuf {
    layout.prefix().join("drive_c").join("Program Files (x86)").join("Funcom").join("The Secret World")
}

/// Detect the installation: a path the user chose by hand wins, then the default locations. Nothing is assumed about the path.
pub fn find_installation(layout: &Layout, state: &State) -> Option<Found> {
    let complete = |client: &Path| client.parent().is_some_and(|d| exists_ci(d, &["RDB", "RDBHashIndex.bin"]));
    if let Some(c) = state.client_exe.as_ref().filter(|c| c.is_file()) {
        return Some(Found { client: c.clone(), complete: true });
    }
    install_roots(layout).iter().find_map(|r| find_client(r)).map(|client| Found { complete: complete(&client), client })
}

// ---- assessment (state derived from the file system) ------------------------------------------------------------------

/// Fingerprint status of the cached installer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Checksum {
    /// Matches the shipped or pinned SHA-256.
    Verified,
    /// A hash is known but nothing to compare it with.
    Unverified,
    /// The hash is still being computed (front ends hash in the background).
    Pending,
}

/// Where an installation stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stage {
    /// Nothing exists below the base yet.
    Empty,
    /// The folders exist; no installer file yet.
    FoldersReady,
    /// A download was interrupted: `bytes` are on disk and will be resumed.
    PartialDownload { bytes: u64 },
    /// The installer file is in the cache.
    InstallerReady { checksum: Checksum },
    /// The prefix exists and was prepared by the launcher.
    PrefixReady,
    /// The installer left only the original patcher (`ClientPatcher.exe`), no complete game yet: the patcher fetches the game data.
    PatcherReady { patcher: PathBuf },
    /// The client and its data exist.
    Installed { client: PathBuf },
}

/// What the user is offered next.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Next {
    PrepareFolders,
    GetInstaller,
    CreatePrefix,
    RunInstaller,
    RunPatcher,
    Play,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Assessment {
    pub stage: Stage,
    pub next: Next,
}

/// Facts-only classification of `layout` (read-only, never hashes a file). `installer_sha`: the SHA-256 of the cached installer, if the
/// caller has it (`None` while a front end is still computing it in the background).
pub fn assess(layout: &Layout, state: &State, installer_sha: Option<&str>) -> Assessment {
    let done = |stage, next| Assessment { stage, next };
    let found = find_installation(layout, state);
    if let Some(f) = found.as_ref().filter(|f| f.complete) {
        return done(Stage::Installed { client: f.client.clone() }, Next::Play);
    }
    if let Some(patcher) = install_roots(layout).iter().find_map(|r| find_file(r, PATCHER_EXE)) {
        return done(Stage::PatcherReady { patcher }, Next::RunPatcher);
    }
    if !layout.prefix().is_dir() || !layout.downloads().is_dir() {
        return done(if layout.base.is_dir() && layout.prefix().is_dir() { Stage::FoldersReady } else { Stage::Empty }, Next::PrepareFolders);
    }
    let prefix_ready = layout.prefix().join("user.reg").is_file() && layout.prefix().join(crate::prefix::MANIFEST).is_file();
    let installer = layout.installer();
    if installer.is_file() {
        if prefix_ready {
            return done(Stage::PrefixReady, Next::RunInstaller);
        }
        let checksum = match installer_sha {
            None => Checksum::Pending,
            Some(h) if state.recognises(&OFFICIAL, h) => Checksum::Verified,
            Some(_) => Checksum::Unverified,
        };
        return done(Stage::InstallerReady { checksum }, Next::CreatePrefix);
    }
    if let Ok(m) = std::fs::metadata(layout.partial()) {
        return done(Stage::PartialDownload { bytes: m.len() }, Next::GetInstaller);
    }
    done(Stage::FoldersReady, Next::GetInstaller)
}

// ---- persisted decisions --------------------------------------------------------------------------------------------------

/// What the user decided. Plain `key = value` text next to the launcher configuration; never a secret.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub base: Option<PathBuf>,
    /// URL the user agreed to download from (consent is for exactly this URL).
    pub download_consent: Option<String>,
    /// SHA-256 the user pinned after verifying the installer out of band.
    pub pinned_sha256: Option<String>,
    /// SHA-256 of the one installer file the user confirmed to run although no trusted checksum exists for it. A different file never inherits this.
    pub confirmed_sha256: Option<String>,
    /// Client executable chosen by hand (manual path selection), used when automatic detection finds nothing.
    pub client_exe: Option<PathBuf>,
}

impl State {
    pub fn parse(text: &str) -> Result<State, String> {
        let mut s = State::default();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let (k, v) = line.split_once('=').ok_or_else(|| format!("install state line {}: expected key = value", i + 1))?;
            let (k, v) = (k.trim(), v.trim());
            match k {
                "base" => s.base = Some(PathBuf::from(v)),
                "download_consent" => s.download_consent = Some(v.to_string()),
                "pinned_sha256" => {
                    if !sha256::is_hex_digest(v) {
                        return Err(format!("install state line {}: pinned_sha256 must be 64 hex digits", i + 1));
                    }
                    s.pinned_sha256 = Some(v.to_ascii_lowercase());
                }
                "confirmed_sha256" => {
                    if !sha256::is_hex_digest(v) {
                        return Err(format!("install state line {}: confirmed_sha256 must be 64 hex digits", i + 1));
                    }
                    s.confirmed_sha256 = Some(v.to_ascii_lowercase());
                }
                "client_exe" => s.client_exe = Some(PathBuf::from(v)),
                // written by a short-lived earlier version (external game folder): read and ignored
                "game_dir" => {}
                other => return Err(format!("install state line {}: unknown key {other:?}", i + 1)),
            }
        }
        Ok(s)
    }

    pub fn to_text(&self) -> String {
        let mut t = String::from("# tsw-launch installation decisions (written by the launcher; no secrets)\n");
        if let Some(b) = &self.base {
            t.push_str(&format!("base = {}\n", b.display()));
        }
        if let Some(c) = &self.download_consent {
            t.push_str(&format!("download_consent = {c}\n"));
        }
        if let Some(p) = &self.pinned_sha256 {
            t.push_str(&format!("pinned_sha256 = {p}\n"));
        }
        if let Some(h) = &self.confirmed_sha256 {
            t.push_str(&format!("confirmed_sha256 = {h}\n"));
        }
        if let Some(c) = &self.client_exe {
            t.push_str(&format!("client_exe = {}\n", c.display()));
        }
        t
    }

    pub fn load(path: &Path) -> Result<State, String> {
        match std::fs::read_to_string(path) {
            Ok(t) => State::parse(&t),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(State::default()),
            Err(e) => Err(format!("cannot read {}: {e}", path.display())),
        }
    }

    /// Atomic save (temporary file + rename); creates the parent directory.
    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
            std::fs::create_dir_all(dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        }
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, self.to_text()).map_err(|e| format!("cannot write {}: {e}", tmp.display()))?;
        std::fs::rename(&tmp, path).map_err(|e| format!("cannot replace {}: {e}", path.display()))
    }

    /// The checksum a download/installer must match: the shipped one, else the one the user pinned.
    pub fn expected_sha256<'a>(&'a self, source: &'a Source) -> Option<&'a str> {
        source.sha256.or(self.pinned_sha256.as_deref())
    }

    /// Is `sha` the shipped checksum of `source` or the one the user pinned? Everything else is an UNKNOWN file (a different build, a repack, a
    /// damaged copy): it is not rejected as such - the user may legitimately own another installer - but it needs a confirmation of exactly
    /// that fingerprint before it runs.
    pub fn recognises(&self, source: &Source, sha: &str) -> bool {
        source.sha256.into_iter().chain(self.pinned_sha256.as_deref()).any(|k| k.eq_ignore_ascii_case(sha))
    }
}

/// The default installation folder: `~/Games/The Secret World` - prefix, runtime, downloads, caches and logs below it, in a Flatpak as well (the sandbox
/// is granted exactly this folder).
pub fn default_base(home: Option<&Path>) -> Option<PathBuf> {
    home.map(|h| h.join("Games").join("The Secret World"))
}

/// Is this base the folder of an EARLIER layout - below the Flatpak's own data folder (`~/.var/app/<id>/data/…`)? Such an installation is only reported
/// ("needs migration"), never used or changed by the launcher.
pub fn is_legacy_base(base: &Path) -> bool {
    let t = base.to_string_lossy();
    t.contains("/.var/app/") && t.contains("/data/")
}

/// Where the state file lives: next to the launcher configuration.
pub fn state_path() -> Option<PathBuf> {
    crate::config::write_path().and_then(|p| p.parent().map(|d| d.join("install-state")))
}

// ---- step 1: folders ------------------------------------------------------------------------------------------------------

/// Create `<base>`, `prefix/`, `runtime/`, `downloads/`, `cache/` and `logs/`. Existing empty folders are fine (idempotent); a non-empty `prefix/`
/// is refused unless it is the very installation described by `assess` (then there is nothing to create).
/// Symbolic links in place of these folders are refused. Nothing outside `<base>` is touched.
pub fn prepare(layout: &Layout) -> Result<(), InstallError> {
    let wanted = [layout.base.clone(), layout.prefix(), layout.runtime(), layout.downloads(), layout.cache(), layout.logs()];
    for d in &wanted {
        if is_symlink(d) {
            return Err(InstallError::BadBase(format!("{} is a symbolic link; choose a real folder", d.display())));
        }
        if d.exists() && !d.is_dir() {
            return Err(InstallError::BadBase(format!("{} exists and is not a folder", d.display())));
        }
    }
    let p = layout.prefix();
    crate::prefix::may_initialise(&p).map_err(InstallError::Blocked)?;
    for d in &wanted {
        std::fs::create_dir_all(d).map_err(|e| failed(&format!("cannot create {}", d.display()), e))?;
    }
    Ok(())
}

// ---- step 2: the installer file ---------------------------------------------------------------------------------------------

/// May the download start? `Ok(expected checksum)` only with consent for exactly this URL, a trusted host and a pinned checksum.
pub fn download_gate(source: &Source, state: &State) -> Result<String, InstallError> {
    let blocked = |m: String| Err(InstallError::Blocked(m));
    let Some(rest) = source.url.strip_prefix("https://") else {
        return blocked("only an https address is allowed as a download source".into());
    };
    let host = rest.split('/').next().unwrap_or("");
    if host != TRUSTED_HOST {
        return blocked(format!("{host:?} is not an allowed download host"));
    }
    if state.download_consent.as_deref() != Some(source.url) {
        return blocked("the download needs your explicit consent for this address".into());
    }
    match state.expected_sha256(source) {
        Some(h) => Ok(h.to_ascii_lowercase()),
        None => blocked(
            "no trusted checksum is known for the installer, so it will not be downloaded automatically. Download the original installer yourself from the official site, \
             check it, and import the file (or pin its SHA-256)"
                .into(),
        ),
    }
}

/// Everything the download needs from outside, injectable for tests.
pub struct Downloader<'a> {
    pub curl: &'a Path,
    pub cancel: &'a AtomicBool,
    pub sink: &'a dyn Sink,
}

fn progress(sink: &dyn Sink, phase: Phase, step: &str, done: Option<u64>, total: Option<u64>, meter: &mut RateMeter, started: Instant) {
    let (rate, eta_secs) = match done {
        Some(d) => meter.sample(d, total, started.elapsed()),
        None => (None, None),
    };
    sink.update(Update::Progress(Progress { phase, step: step.into(), done, total, rate, eta_secs }));
}

/// Download the installer into the cache. Resumes a `.part` file; the file gets its final name only after the SHA-256 matched.
/// A mismatching or unreadable result is removed (it is the launcher's own cache file) and reported. Cancel keeps the `.part`.
pub fn download(layout: &Layout, source: &Source, state: &State, dl: &Downloader) -> Result<PathBuf, InstallError> {
    let result = run_download(layout, source, state, dl);
    dl.sink.update(Update::Done(match &result {
        Ok(_) => Outcome::Finished("The installer was downloaded and verified.".into()),
        Err(e) => Outcome::Failed(e.to_string()),
    }));
    result
}

fn run_download(layout: &Layout, source: &Source, state: &State, dl: &Downloader) -> Result<PathBuf, InstallError> {
    let expected = download_gate(source, state)?;
    if !layout.downloads().is_dir() || is_symlink(&layout.downloads()) {
        return Err(InstallError::Failed("the download folder does not exist yet; prepare the folders first".into()));
    }
    let (part, dest) = (layout.partial(), layout.installer());
    if dest.is_file() {
        return verify_final(&dest, &expected).map(|_| dest);
    }
    fetch_verified(dl, source.url, &part, &dest, &expected, "Downloading the installer")
}

/// Download `url` to `part` (resumed if it exists), verify its SHA-256 and only then give it the final name `dest`. A mismatching result is
/// removed (it is the launcher's own cache file). Shared by the installer download and the runner download; https only, certificates always checked.
pub(crate) fn fetch_verified(dl: &Downloader, url: &str, part: &Path, dest: &Path, expected: &str, step: &str) -> Result<PathBuf, InstallError> {
    let total = head_length(dl.curl, url);
    let started = Instant::now();
    let mut meter = RateMeter::default();
    for attempt in 0..2 {
        let mut cmd = Command::new(dl.curl);
        // `-C -` resumes at the size of the .part file; redirects are limited and only https is accepted, also after a redirect
        // address of the official host; certificate checks are never turned off.
        cmd.args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--max-redirs",
            "3",
            "--proto",
            "=https",
            "--proto-redir",
            "=https",
            "--connect-timeout",
            "20",
            "--speed-limit",
            "1000",
            "--speed-time",
            "60",
            "-C",
            "-",
            "--output",
        ])
        .arg(part)
        .arg("--")
        .arg(url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
        let mut child = cmd.spawn().map_err(|e| failed(&format!("cannot start {}", dl.curl.display()), e))?;
        let status = loop {
            if dl.cancel.load(Ordering::SeqCst) {
                let _ = child.kill();
                let _ = child.wait();
                return Err(InstallError::Cancelled);
            }
            if let Some(s) = child.try_wait().map_err(|e| failed("waiting for curl", e))? {
                break s;
            }
            let done = std::fs::metadata(part).map(|m| m.len()).ok();
            progress(dl.sink, Phase::Downloading, step, done, total, &mut meter, started);
            std::thread::sleep(Duration::from_millis(100));
        };
        if status.success() {
            break;
        }
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = io::Read::read_to_string(&mut s, &mut err);
        }
        // exit 33: the server cannot resume from this offset (or the .part is stale): start over once, from our own cache file only
        if status.code() == Some(33) && attempt == 0 {
            let _ = std::fs::remove_file(part);
            continue;
        }
        return Err(InstallError::Failed(format!("download failed ({status}): {}", err.trim())));
    }
    dl.sink.update(Update::Progress(Progress::indeterminate(Phase::Verifying, "Checking the SHA-256 checksum")));
    match verify_final(part, expected) {
        Ok(()) => {
            std::fs::rename(part, dest).map_err(|e| failed("cannot move the verified installer into place", e))?;
            Ok(dest.to_path_buf())
        }
        Err(e) => {
            let _ = std::fs::remove_file(part);
            Err(e)
        }
    }
}

fn verify_final(file: &Path, expected: &str) -> Result<(), InstallError> {
    let (hash, _) = sha256::file_sha256(file).map_err(|e| failed(&format!("cannot read {}", file.display()), e))?;
    if hash.eq_ignore_ascii_case(expected) {
        Ok(())
    } else {
        Err(InstallError::Failed(format!("checksum mismatch: the file has SHA-256 {hash}, expected {expected}; it was discarded")))
    }
}

/// Best effort: the size announced by the server (for a real progress percentage). `None` if unknown; never an error.
fn head_length(curl: &Path, url: &str) -> Option<u64> {
    let out = Command::new(curl)
        .args(["--head", "--silent", "--fail", "--location", "--max-redirs", "3", "--proto", "=https", "--proto-redir", "=https", "--connect-timeout", "10", "--max-time", "20", "--"])
        .arg(url)
        .stdin(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).lines().rev().find_map(|l| l.to_ascii_lowercase().strip_prefix("content-length:").and_then(|v| v.trim().parse().ok()))).flatten()
}

/// Use an installer file the user obtained and checked themselves: it is COPIED into the cache (the original is only read).
/// With a known checksum it must match; without one it is accepted as "unverified" and its SHA-256 is returned for the user to compare.
pub fn import_installer(layout: &Layout, from: &Path, expected: Option<&str>) -> Result<(PathBuf, String), InstallError> {
    if !layout.downloads().is_dir() || is_symlink(&layout.downloads()) {
        return Err(InstallError::Failed("the download folder does not exist yet; prepare the folders first".into()));
    }
    if !from.is_file() {
        return Err(InstallError::Failed(format!("{} is not a file", from.display())));
    }
    let (dest, tmp) = (layout.installer(), layout.downloads().join(format!("{INSTALLER_NAME}.import")));
    if dest.exists() {
        return Err(InstallError::NotEmpty(dest));
    }
    std::fs::copy(from, &tmp).map_err(|e| failed("cannot copy the installer", e))?;
    let (hash, _) = sha256::file_sha256(&tmp).map_err(|e| failed("cannot read the copied installer", e))?;
    if let Some(exp) = expected {
        if !hash.eq_ignore_ascii_case(exp) {
            let _ = std::fs::remove_file(&tmp);
            return Err(InstallError::Failed(format!("checksum mismatch: the file has SHA-256 {hash}, expected {exp}")));
        }
    }
    std::fs::rename(&tmp, &dest).map_err(|e| failed("cannot move the installer into place", e))?;
    Ok((dest, hash))
}

// ---- step 3: prefix ---------------------------------------------------------------------------------------------------------

/// Create the prefix with the launcher's own prefix manager (reproducible: same code path as `tsw-launch prefix init`). The original
/// installer is then run in it with its default Windows paths. Refuses a non-empty prefix folder that the launcher did not create.
pub fn create_prefix(layout: &Layout, wine: &Path, sink: &dyn Sink) -> Result<(), InstallError> {
    let p = layout.prefix();
    if is_symlink(&p) || !p.is_dir() {
        return Err(InstallError::Failed("prepare the folders first".into()));
    }
    crate::prefix::may_initialise(&p).map_err(InstallError::Blocked)?;
    crate::prefix::init_with(&p, wine, sink).map(|_| ()).map_err(InstallError::Failed)
}

// ---- step 4: run the original installer --------------------------------------------------------------------------------------

/// May the installer be started? It must exist and the prefix must be ready. `installer_sha` is the file's SHA-256 (computed by the caller, off the UI
/// thread). A file that matches the shipped/pinned checksum runs; a different pinned checksum always blocks; an unknown file needs the user's
/// confirmation of exactly this fingerprint.
pub fn run_gate(layout: &Layout, state: &State, source: &Source, installer_sha: &str) -> Result<(), InstallError> {
    if !layout.installer().is_file() {
        return Err(InstallError::Failed("there is no installer in the cache yet".into()));
    }
    if !layout.prefix().join("user.reg").is_file() {
        return Err(InstallError::Failed("create the prefix first".into()));
    }
    // a checksum the user pinned explicitly is an expectation: a different file is a mismatch, never silently accepted
    if let Some(pin) = state.pinned_sha256.as_deref().filter(|p| !p.eq_ignore_ascii_case(installer_sha)) {
        if !source.sha256.is_some_and(|s| s.eq_ignore_ascii_case(installer_sha)) {
            return Err(InstallError::Failed(format!("checksum mismatch: the installer has SHA-256 {installer_sha}, expected {pin}")));
        }
    }
    match () {
        _ if state.recognises(source, installer_sha) => Ok(()),
        _ if state.confirmed_sha256.as_deref().is_some_and(|c| c.eq_ignore_ascii_case(installer_sha)) => Ok(()),
        _ => Err(InstallError::Blocked(format!("this installer file (SHA-256 {installer_sha}) is not known to be genuine; confirm exactly this fingerprint after you checked it"))),
    }
}

/// The command that runs the original installer interactively in the prefix: `<wine> <installer>` in the cache folder with
/// `WINEPREFIX` set; menu-entry creation and Mono/Gecko downloads are switched off so nothing is written outside `<base>`.
pub fn installer_command(layout: &Layout, wine: &Path) -> Command {
    let mut c = Command::new(wine);
    c.arg(layout.installer()).current_dir(layout.downloads()).env("WINEPREFIX", layout.prefix()).env("WINEDLLOVERRIDES", "mscoree=d;mshtml=d;winemenubuilder.exe=d").env("WINEDEBUG", "-all");
    c
}

/// The command that runs the original patcher (which downloads the game data) in the prefix, in the patcher's own folder.
/// Started only on the user's explicit click; the patcher itself talks to the network, the launcher does not.
/// The original patcher to start for this stage: after the installer left only the patcher, and ALSO once the client exists (the patcher fetches and
/// resumes the game data itself, so it must stay startable until the user decides the installation is complete). `None` = no patcher beside the client.
pub fn patcher_to_run(stage: &Stage) -> Option<PathBuf> {
    match stage {
        Stage::PatcherReady { patcher } => Some(patcher.clone()),
        Stage::Installed { client } => client.parent().map(|d| d.join(PATCHER_EXE)).filter(|p| p.is_file()),
        _ => None,
    }
}

pub fn patcher_command(layout: &Layout, wine: &Path, patcher: &Path) -> Command {
    let mut c = Command::new(wine);
    c.arg(patcher)
        .current_dir(patcher.parent().unwrap_or(&layout.base))
        .env("WINEPREFIX", layout.prefix())
        .env("WINEDLLOVERRIDES", "mscoree=d;mshtml=d;winemenubuilder.exe=d")
        .env("WINEDEBUG", "-all");
    c
}

// ---- RDB import (a game data set the user already owns) ---------------------------------------------------------------------

/// What [`import_rdb`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RdbImport {
    pub copied: Vec<String>,
    pub skipped_existing: Vec<String>,
    pub bytes: u64,
    /// `true` when every file was cloned (btrfs reflink: instant, no extra space); `false` when full copies were made (only with `allow_copy`).
    pub reflinked: bool,
}

/// Bring an existing `RDB` folder (for example from another installation of the user's own) into this installation:
/// `<prefix>/drive_c/Program Files (x86)/Funcom/The Secret World/RDB`. The SOURCE is whatever the user names - nothing is searched, and nothing is ever
/// changed in it. Rules: the source must be a real folder (no link) outside the managed folder, holding only regular files; an existing destination
/// file is skipped unless `overwrite` is set (the user's confirmation); files are cloned with `cp --reflink=always` (btrfs: instant, no extra space)
/// and, where that is impossible, NOT copied unless `allow_copy` is set (the data set is ~40 GB); each file appears under its final name only when complete.
/// The result is not declared complete: the original ClientPatcher verifies and completes it.
pub fn import_rdb(layout: &Layout, source: &Path, overwrite: bool, allow_copy: bool) -> Result<RdbImport, InstallError> {
    let bad = |m: String| Err(InstallError::Failed(m));
    if !source.is_absolute() || source.to_string_lossy().split('/').any(|c| c == ".." || c == ".") {
        return bad("the RDB source must be an absolute path without '.' or '..'".into());
    }
    if is_symlink(source) || !source.is_dir() {
        return bad(format!("{} is not a real folder (links are not followed)", source.display()));
    }
    if source.starts_with(&layout.base) {
        return bad("the source lies inside the installation folder; name the folder you want to take the data from".into());
    }
    if !layout.prefix().join("user.reg").is_file() {
        return bad("create the prefix first".into());
    }
    let mut files = Vec::new();
    for e in std::fs::read_dir(source).map_err(|e| failed("cannot read the source folder", e))?.flatten() {
        let ft = e.file_type().map_err(|e| failed("cannot inspect the source", e))?;
        if !ft.is_file() {
            return bad(format!("{} is not a regular file (only plain files are taken over)", e.path().display()));
        }
        files.push(e.path());
    }
    if files.is_empty() {
        return bad("the source folder is empty".into());
    }
    files.sort();
    let dest_dir = game_path(layout).join("RDB");
    std::fs::create_dir_all(&dest_dir).map_err(|e| failed("cannot create the RDB folder", e))?;
    let mut report = RdbImport { reflinked: true, ..RdbImport::default() };
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().into_owned();
        let dest = dest_dir.join(&name);
        if dest.exists() && !overwrite {
            report.skipped_existing.push(name);
            continue;
        }
        let tmp = dest_dir.join(format!("{name}.import-part"));
        let _ = std::fs::remove_file(&tmp);
        let run = |mode: &str| Command::new("cp").arg(format!("--reflink={mode}")).arg("--preserve=timestamps").arg("--").arg(f).arg(&tmp).stdin(Stdio::null()).output();
        let mut out = run("always").map_err(|e| failed("cannot start cp", e))?;
        if !out.status.success() {
            let _ = std::fs::remove_file(&tmp);
            if !allow_copy {
                return bad(format!(
                    "{name} cannot be cloned (no reflink between these folders). A full copy of a ~40 GB data set was NOT made; pass --allow-copy to make it. {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                ));
            }
            report.reflinked = false;
            out = run("never").map_err(|e| failed("cannot start cp", e))?;
            if !out.status.success() {
                let _ = std::fs::remove_file(&tmp);
                return bad(format!("copying {name} failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
        }
        let size = std::fs::metadata(&tmp).map(|m| m.len()).unwrap_or(0);
        std::fs::rename(&tmp, &dest).map_err(|e| failed("cannot move the imported file into place", e))?;
        report.bytes += size;
        report.copied.push(name);
    }
    Ok(report)
}

// ---- finishing / adopting ---------------------------------------------------------------------------------------------------

/// Settings to store once an installation exists. `None` fields are left as the user has them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Adoption {
    pub client_exe: PathBuf,
    pub prefix: Option<PathBuf>,
}

/// Resolve what the user pointed at (the client executable, or a folder that contains it) without changing anything.
pub fn adopt(path: &Path) -> Result<PathBuf, InstallError> {
    let exe = if path.is_dir() { find_client(path) } else { path.is_file().then(|| path.to_path_buf()) };
    let exe = exe.ok_or_else(|| InstallError::Failed(format!("no {CLIENT_EXE} found at {}", path.display())))?;
    if !exe.file_name().is_some_and(|n| n.to_string_lossy().eq_ignore_ascii_case(CLIENT_EXE)) {
        return Err(InstallError::Failed(format!("{} is not {CLIENT_EXE}", exe.display())));
    }
    Ok(exe)
}

/// Merge a finished/adopted installation into the launcher configuration, protecting a working one: an existing `client_exe`
/// that still points to a file is kept and reported as a conflict instead of being replaced.
pub fn merge_config(cfg: &mut crate::config::Config, a: &Adoption) -> Result<(), InstallError> {
    if let Some(existing) = cfg.client_exe.as_ref().filter(|p| p.is_file() && **p != a.client_exe) {
        return Err(InstallError::Blocked(format!(
            "the configuration already points to a working installation ({}); it was not changed. Remove client_exe from the configuration first if you want to switch.",
            existing.display()
        )));
    }
    cfg.client_exe = Some(a.client_exe.clone());
    if let Some(p) = &a.prefix {
        if cfg.prefix.as_ref().is_none_or(|old| !old.is_dir()) {
            cfg.prefix = Some(p.clone());
        }
    }
    Ok(())
}

/// The adoption that belongs to a finished installation under `layout`.
pub fn adoption_for(layout: &Layout, state: &State) -> Option<Adoption> {
    find_installation(layout, state).filter(|f| f.complete).map(|f| Adoption { client_exe: f.client, prefix: layout.prefix().join("user.reg").is_file().then(|| layout.prefix()) })
}

// ---- tests ------------------------------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    /// The shipped source without its pin: most tests pin their own fixture checksum.
    const UNPINNED: Source = Source { sha256: None, ..OFFICIAL };
    use super::*;
    use crate::progress::Silent;

    fn tmp(tag: &str) -> PathBuf {
        static N: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!("tsw-install-{tag}-{}-{}", std::process::id(), N.fetch_add(1, Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn layout(tag: &str) -> (PathBuf, Layout) {
        let root = tmp(tag);
        let l = Layout::new(&root.join("Games").join("TheSecretWorld"), Some(&root.join("home"))).unwrap();
        (root, l)
    }

    #[cfg(unix)]
    fn script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    #[test]
    fn base_folders_are_validated_without_touching_anything() {
        let _exec = crate::test_exec_lock();
        let home = Path::new("/home/someone");
        for ok in ["/home/someone/Games/TheSecretWorld", "/mnt/data/tsw", "/home/other/tsw"] {
            assert!(Layout::new(Path::new(ok), Some(home)).is_ok(), "{ok}");
        }
        for bad in ["relative/path", "/", "/home", "/tmp", "/usr/share/tsw", "/etc/tsw", "/home/someone", "/home/someone/Games/../..", "/home/someone/./x", "/mnt"] {
            assert!(matches!(Layout::new(Path::new(bad), Some(home)), Err(InstallError::BadBase(_))), "{bad}");
        }
        assert!(Layout::new(Path::new("/home"), None).is_err(), "no home known: still not a top-level folder");
        let l = Layout::new(Path::new("/mnt/data/tsw"), Some(home)).unwrap();
        assert_eq!((l.prefix(), l.downloads(), l.runtime()), (PathBuf::from("/mnt/data/tsw/prefix"), PathBuf::from("/mnt/data/tsw/downloads"), PathBuf::from("/mnt/data/tsw/runtime")));
    }

    #[test]
    fn preparing_creates_the_five_folders_and_only_those_and_is_idempotent() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("prep");
        assert_eq!(assess(&l, &State::default(), None), Assessment { stage: Stage::Empty, next: Next::PrepareFolders });
        prepare(&l).unwrap();
        prepare(&l).unwrap();
        let mut names: Vec<_> = std::fs::read_dir(&l.base).unwrap().flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect();
        names.sort();
        assert_eq!(names, ["cache", "downloads", "logs", "prefix", "runtime"], "exactly these folders; no separate game folder: the game is installed inside the prefix");
        assert_eq!(assess(&l, &State::default(), None), Assessment { stage: Stage::FoldersReady, next: Next::GetInstaller });
        assert!(!root.join("home").exists(), "nothing outside the base folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_existing_installation_or_foreign_data_is_never_overwritten() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("exist");
        let dir = l.prefix().join("drive_c/Program Files (x86)/Funcom/The Secret World");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(CLIENT_EXE), b"MZ").unwrap();
        std::fs::write(l.prefix().join("user.reg"), b"x").unwrap();
        let a = assess(&l, &State::default(), None);
        assert!(!matches!(a.stage, Stage::Installed { .. }), "the client without its data (RDB/RDBHashIndex.bin) is not a finished installation");
        std::fs::create_dir_all(dir.join("RDB")).unwrap();
        std::fs::write(dir.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        let a = assess(&l, &State::default(), None);
        assert_eq!((a.stage, a.next), (Stage::Installed { client: dir.join(CLIENT_EXE) }, Next::Play), "found INSIDE the prefix, path detected not assumed");
        assert!(!l.base.join("game").exists(), "no second copy and no separate game folder");
        assert!(matches!(prepare(&l), Err(InstallError::Blocked(m)) if m.contains("not a proven TSW-Forever prefix")), "existing game files do not establish prefix ownership");
        assert_eq!(std::fs::read(dir.join(CLIENT_EXE)).unwrap(), b"MZ", "an existing installation is untouched");
        // a manually chosen client wins and counts as installed
        let manual = root.join("elsewhere/TheSecretWorldDX11.exe");
        std::fs::create_dir_all(manual.parent().unwrap()).unwrap();
        std::fs::write(&manual, b"MZ").unwrap();
        let st = State { client_exe: Some(manual.clone()), ..State::default() };
        assert_eq!(assess(&l, &st, None).stage, Stage::Installed { client: manual });
        // a base with foreign, non-empty game/ (no client in it) is refused
        let (root2, l2) = layout("foreign");
        std::fs::create_dir_all(l2.prefix()).unwrap();
        std::fs::write(l2.prefix().join("other"), b"x").unwrap();
        assert!(matches!(prepare(&l2), Err(InstallError::Blocked(m)) if m.contains("not a proven TSW-Forever prefix")), "a foreign prefix folder is not ours");
        assert!(!l2.downloads().exists(), "a refused preparation creates nothing");
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&root2));
    }

    #[cfg(unix)]
    #[test]
    fn symbolic_links_in_place_of_the_folders_are_refused() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("link");
        std::fs::create_dir_all(&l.base).unwrap();
        let elsewhere = root.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::os::unix::fs::symlink(&elsewhere, l.cache()).unwrap();
        assert!(matches!(prepare(&l), Err(InstallError::BadBase(_))));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_client_is_found_in_nested_install_folders_only_by_its_exact_name() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("find");
        let deep = game_path(&l);
        std::fs::create_dir_all(&deep).unwrap();
        assert_eq!(find_client(&l.prefix().join("drive_c")), None);
        std::fs::write(deep.join("TheSecretWorld.exe"), b"x").unwrap();
        assert_eq!(find_client(&l.prefix().join("drive_c")), None, "the DX9 name is not the launcher's client");
        std::fs::write(deep.join("thesecretworlddx11.EXE"), b"x").unwrap();
        assert!(find_client(&l.prefix().join("drive_c")).is_some(), "case-insensitive, like Windows");
        assert_eq!(adopt(&game_path(&l)).unwrap(), find_client(&l.prefix().join("drive_c")).unwrap());
        assert!(adopt(&deep.join("TheSecretWorld.exe")).is_err() && adopt(&root.join("nothing")).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_state_file_round_trips_validates_and_a_missing_file_is_an_empty_state() {
        let _exec = crate::test_exec_lock();
        let (root, _) = layout("state");
        let p = root.join("cfg").join("install-state");
        assert_eq!(State::load(&p).unwrap(), State::default());
        let s = State {
            base: Some("/mnt/x y/tsw".into()),
            download_consent: Some(UNPINNED.url.into()),
            pinned_sha256: Some("ab".repeat(32)),
            confirmed_sha256: Some("cd".repeat(32)),
            client_exe: Some("/g/c.exe".into()),
        };
        s.save(&p).unwrap();
        assert_eq!(State::load(&p).unwrap(), s);
        assert!(!p.with_extension("tmp").exists());
        for bad in ["pinned_sha256 = xyz\n", "colour = red\n", "no equals sign\n"] {
            assert!(State::parse(bad).is_err(), "{bad:?}");
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_download_is_blocked_without_consent_without_a_trusted_checksum_and_for_foreign_hosts() {
        let _exec = crate::test_exec_lock();
        let consent = State { download_consent: Some(OFFICIAL.url.into()), ..State::default() };
        assert!(matches!(download_gate(&OFFICIAL, &State::default()), Err(InstallError::Blocked(m)) if m.contains("consent")));
        let unpinned = Source { sha256: None, ..OFFICIAL };
        let blocked = download_gate(&unpinned, &consent).unwrap_err();
        assert!(matches!(&blocked, InstallError::Blocked(m) if m.contains("no trusted checksum")), "{blocked}");
        assert!(OFFICIAL.url.starts_with("https://"), "the shipped source is https");
        assert_eq!(OFFICIAL.sha256.map(str::len), Some(64), "the shipped pin is a SHA-256");
        let plain = Source { url: "http://cdn.funcom.com/downloads/tsw/client/TheSecretWorldInstaller.exe", ..OFFICIAL };
        assert!(
            matches!(download_gate(&plain, &State { download_consent: Some(plain.url.into()), ..State::default() }), Err(InstallError::Blocked(m)) if m.contains("https")),
            "plain http is refused"
        );
        let pinned = State { pinned_sha256: Some("AB".repeat(32)), ..consent.clone() };
        assert_eq!(download_gate(&unpinned, &pinned).unwrap(), "ab".repeat(32));
        let other = Source { url: "https://evil.example/TheSecretWorldInstaller.exe", sha256: Some("00"), origin: "test" };
        assert!(matches!(download_gate(&other, &State { download_consent: Some(other.url.into()), ..State::default() }), Err(InstallError::Blocked(m)) if m.contains("not an allowed")));
        let sneaky = Source { url: "https://cdn.funcom.com.evil.example/x.exe", sha256: Some("00"), origin: "test" };
        assert!(download_gate(&sneaky, &State { download_consent: Some(sneaky.url.into()), ..State::default() }).is_err());
        let shipped = Source { sha256: Some("cd".repeat(32).leak()), ..OFFICIAL };
        assert!(download_gate(&shipped, &State::default()).is_err(), "even a shipped checksum needs the user's consent");
        assert!(download_gate(&shipped, &consent).is_ok());
        let wrong_url = State { download_consent: Some("https://cdn.funcom.com/other.exe".into()), ..State::default() };
        assert!(download_gate(&shipped, &wrong_url).is_err(), "consent is for exactly one address");
    }

    /// A fake `curl` that "downloads" a fixture, honours `-C -` (resume from the size of the output file) and records its arguments.
    #[cfg(unix)]
    fn fake_curl(dir: &Path, payload: &Path, fail_resume: bool) -> PathBuf {
        let curl = dir.join("curl");
        let log = dir.join("curl-args.log");
        script(
            &curl,
            &format!(
                r#"echo "$@" >> '{log}'
case " $* " in *" --head "*) printf 'HTTP/1.1 200 OK\r\nContent-Length: %s\r\n\r\n' "$(wc -c < '{p}')"; exit 0;; esac
out=""; prev=""; for a in "$@"; do if [ "$prev" = "--output" ]; then out="$a"; fi; prev="$a"; done
have=0; [ -f "$out" ] && have=$(wc -c < "$out")
if [ "$have" -gt 0 ] && [ "{fail}" = "1" ]; then echo "range not satisfiable" >&2; exit 33; fi
tail -c +$((have+1)) '{p}' >> "$out""#,
                log = log.display(),
                p = payload.display(),
                fail = if fail_resume { 1 } else { 0 }
            ),
        );
        curl
    }

    #[cfg(unix)]
    fn consenting(hash: &str) -> State {
        State { download_consent: Some(UNPINNED.url.into()), pinned_sha256: Some(hash.into()), ..State::default() }
    }

    #[cfg(unix)]
    #[test]
    fn a_pinned_download_is_verified_before_it_gets_its_final_name_and_never_touches_the_network_in_tests() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("dl");
        prepare(&l).unwrap();
        let payload = root.join("payload.bin");
        std::fs::write(&payload, vec![7u8; 300_000]).unwrap();
        let hash = sha256::sha256_hex(&std::fs::read(&payload).unwrap());
        let curl = fake_curl(&root, &payload, false);
        let cancel = AtomicBool::new(false);
        let seen = std::sync::Mutex::new(Vec::<Update>::new());
        let sink = |u: Update| seen.lock().unwrap().push(u);
        let dest = download(&l, &UNPINNED, &consenting(&hash), &Downloader { curl: &curl, cancel: &cancel, sink: &sink }).unwrap();
        assert_eq!(dest, l.installer());
        assert!(!l.partial().exists(), "the .part file became the final file");
        assert_eq!(assess(&l, &consenting(&hash), Some(&hash)).stage, Stage::InstallerReady { checksum: Checksum::Verified });
        assert!(matches!(seen.lock().unwrap().last(), Some(Update::Done(Outcome::Finished(_)))), "exactly one final update");
        let args = std::fs::read_to_string(root.join("curl-args.log")).unwrap();
        assert!(args.contains(UNPINNED.url) && args.contains("-C -") && args.contains("--proto =https"), "{args}");
        assert!(!args.contains("--insecure") && !args.contains(" -k"), "certificate checks stay on");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_wrong_checksum_discards_the_download_and_blocked_downloads_never_start_curl() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("dlbad");
        prepare(&l).unwrap();
        let payload = root.join("payload.bin");
        std::fs::write(&payload, b"not the installer").unwrap();
        let curl = fake_curl(&root, &payload, false);
        let cancel = AtomicBool::new(false);
        let dl = Downloader { curl: &curl, cancel: &cancel, sink: &Silent };
        let err = download(&l, &UNPINNED, &consenting(&"00".repeat(32)), &dl).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch"), "{err}");
        assert!(!l.partial().exists() && !l.installer().exists(), "nothing unverified stays behind");
        let _ = std::fs::remove_file(root.join("curl-args.log"));
        assert!(matches!(download(&l, &UNPINNED, &State { download_consent: Some(UNPINNED.url.into()), ..State::default() }, &dl), Err(InstallError::Blocked(_))));
        assert!(matches!(download(&l, &UNPINNED, &State::default(), &dl), Err(InstallError::Blocked(_))));
        assert!(!root.join("curl-args.log").exists(), "curl was never started for a blocked download");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn an_interrupted_download_is_resumed_and_a_server_that_cannot_resume_restarts_cleanly() {
        let _exec = crate::test_exec_lock();
        for (tag, fail_resume) in [("resume", false), ("noresume", true)] {
            let (root, l) = layout(tag);
            prepare(&l).unwrap();
            let payload = root.join("payload.bin");
            let data: Vec<u8> = (0..200_000u32).map(|i| (i % 251) as u8).collect();
            std::fs::write(&payload, &data).unwrap();
            let hash = sha256::sha256_hex(&data);
            std::fs::write(l.partial(), &data[..70_000]).unwrap();
            assert_eq!(assess(&l, &State::default(), None), Assessment { stage: Stage::PartialDownload { bytes: 70_000 }, next: Next::GetInstaller });
            let curl = fake_curl(&root, &payload, fail_resume);
            let cancel = AtomicBool::new(false);
            download(&l, &UNPINNED, &consenting(&hash), &Downloader { curl: &curl, cancel: &cancel, sink: &Silent }).unwrap();
            assert_eq!(std::fs::read(l.installer()).unwrap(), data, "{tag}: the finished file is byte-identical");
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[cfg(unix)]
    #[test]
    fn cancelling_stops_curl_and_keeps_the_partial_file_for_the_next_try() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("cancel");
        prepare(&l).unwrap();
        let curl = root.join("slow-curl");
        script(&curl, "case \" $* \" in *\" --head \"*) exit 1;; esac\nout=\"\"; prev=\"\"; for a in \"$@\"; do if [ \"$prev\" = \"--output\" ]; then out=\"$a\"; fi; prev=\"$a\"; done\nprintf 'partial' > \"$out\"\nsleep 30");
        let cancel = std::sync::Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        let t = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(600));
            c2.store(true, Ordering::SeqCst);
        });
        let started = Instant::now();
        let err = download(&l, &UNPINNED, &consenting(&"11".repeat(32)), &Downloader { curl: &curl, cancel: &cancel, sink: &Silent }).unwrap_err();
        t.join().unwrap();
        assert_eq!(err, InstallError::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(10), "curl was killed, not waited for");
        assert_eq!(std::fs::read(l.partial()).unwrap(), b"partial", "the partial file is kept");
        assert!(!l.installer().exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn a_curl_failure_is_reported_with_its_message_and_leaves_no_final_file() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("curlfail");
        prepare(&l).unwrap();
        let curl = root.join("bad-curl");
        script(&curl, "case \" $* \" in *\" --head \"*) exit 1;; esac\necho 'Could not resolve host' >&2\nexit 6");
        let err = download(&l, &UNPINNED, &consenting(&"11".repeat(32)), &Downloader { curl: &curl, cancel: &AtomicBool::new(false), sink: &Silent }).unwrap_err();
        assert!(err.to_string().contains("Could not resolve host"), "{err}");
        assert!(!l.installer().exists());
        let missing = download(&l, &UNPINNED, &consenting(&"11".repeat(32)), &Downloader { curl: &root.join("no-such-curl"), cancel: &AtomicBool::new(false), sink: &Silent }).unwrap_err();
        assert!(matches!(missing, InstallError::Failed(m) if m.contains("cannot start")));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn importing_copies_the_users_file_checks_a_known_checksum_and_never_replaces_the_cache_file() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("import");
        prepare(&l).unwrap();
        let src = root.join("my installer.exe");
        std::fs::write(&src, b"installer bytes").unwrap();
        let good = sha256::sha256_hex(b"installer bytes");
        let err = import_installer(&l, &src, Some(&"00".repeat(32))).unwrap_err();
        assert!(err.to_string().contains("checksum mismatch") && !l.installer().exists() && !l.downloads().join(format!("{INSTALLER_NAME}.import")).exists());
        let (dest, hash) = import_installer(&l, &src, None).unwrap();
        assert_eq!((dest.as_path(), hash.as_str()), (l.installer().as_path(), good.as_str()));
        assert_eq!(std::fs::read(&src).unwrap(), b"installer bytes", "the original is untouched");
        assert!(matches!(import_installer(&l, &src, None), Err(InstallError::NotEmpty(_))), "an installer in the cache is never replaced");
        assert_eq!(assess(&l, &State::default(), Some(&good)).stage, Stage::InstallerReady { checksum: Checksum::Unverified }, "no pin: unverified");
        assert_eq!(assess(&l, &State::default(), None).stage, Stage::InstallerReady { checksum: Checksum::Pending }, "hash not computed yet");
        let pin = State { pinned_sha256: Some(good.clone()), ..State::default() };
        assert_eq!(assess(&l, &pin, Some(&good)).stage, Stage::InstallerReady { checksum: Checksum::Verified });
        assert!(import_installer(&l, &root.join("missing.exe"), None).is_err());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[cfg(unix)]
    #[test]
    fn the_prefix_is_created_by_the_prefix_manager_and_foreign_prefixes_are_refused() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("prefix");
        prepare(&l).unwrap();
        // a stand-in runtime: <rt>/files/bin/wine that creates user.reg like wineboot would, plus the components the manager copies
        let rt = root.join("rt");
        std::fs::create_dir_all(rt.join("files/bin")).unwrap();
        let ops = crate::prefix::plan_components(&rt.join("files"), &l.prefix());
        for op in &ops {
            std::fs::create_dir_all(op.from.parent().unwrap()).unwrap();
            std::fs::write(&op.from, b"dll").unwrap();
        }
        let wine = rt.join("files/bin/wine");
        script(&wine, "case \"$1\" in --version) echo wine-test;; wineboot) mkdir -p \"$WINEPREFIX/drive_c/windows/system32\" \"$WINEPREFIX/drive_c/windows/syswow64\"; printf '[Control Panel\\\\\\\\Desktop\\\\\\\\WindowMetrics] 1\\n\"CaptionHeight\"=\"25\"\\n' > \"$WINEPREFIX/user.reg\";; esac");
        script(&rt.join("files/bin/wineserver"), "exit 0");
        // Interrupt the component phase after Wine created the registry. Only our prior marker permits retry.
        let interrupted = &ops[0].from;
        std::fs::remove_file(interrupted).unwrap();
        assert!(create_prefix(&l, &wine, &Silent).is_err());
        assert!(l.prefix().join("user.reg").is_file());
        assert!(!l.prefix().join(crate::prefix::MANIFEST).exists());
        std::fs::write(interrupted, b"dll").unwrap();
        create_prefix(&l, &wine, &Silent).unwrap();
        assert!(l.prefix().join(crate::prefix::MANIFEST).is_file() && l.prefix().join("user.reg").is_file());
        assert!(!l.prefix().join("dosdevices/g:").exists(), "no extra drive for a separate game folder any more");
        assert_eq!(assess(&l, &State::default(), None).stage, Stage::FoldersReady, "without an installer file the stage stays at the folders");
        // second call is harmless
        create_prefix(&l, &wine, &Silent).unwrap();
        // a foreign, non-empty prefix folder is refused before anything runs
        let (root2, l2) = layout("prefix-foreign");
        std::fs::create_dir_all(l2.prefix()).unwrap();
        std::fs::write(l2.prefix().join("mine.txt"), b"x").unwrap();
        assert!(matches!(create_prefix(&l2, &wine, &Silent), Err(InstallError::Blocked(m)) if m.contains("not a proven TSW-Forever prefix")));
        assert!(!l2.prefix().join("user.reg").exists());
        let _ = (std::fs::remove_dir_all(&root), std::fs::remove_dir_all(&root2));
    }

    #[test]
    fn running_the_installer_needs_the_file_the_prefix_and_for_unknown_files_a_confirmation_bound_to_that_exact_file() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("run");
        prepare(&l).unwrap();
        let st = State::default();
        let sha = sha256::sha256_hex(b"exe");
        assert!(run_gate(&l, &st, &UNPINNED, &sha).is_err(), "no installer yet");
        std::fs::write(l.installer(), b"exe").unwrap();
        assert!(run_gate(&l, &st, &UNPINNED, &sha).is_err(), "no prefix yet");
        std::fs::write(l.prefix().join("user.reg"), b"x").unwrap();
        let blocked = run_gate(&l, &st, &UNPINNED, &sha).unwrap_err();
        assert!(matches!(&blocked, InstallError::Blocked(m) if m.contains(&sha)), "the message shows the fingerprint to confirm: {blocked}");
        assert!(run_gate(&l, &State { confirmed_sha256: Some(sha.clone()), ..State::default() }, &UNPINNED, &sha).is_ok());
        // the confirmation belongs to ONE file: another installer does not inherit it
        let other = sha256::sha256_hex(b"another file");
        assert!(run_gate(&l, &State { confirmed_sha256: Some(sha.clone()), ..State::default() }, &UNPINNED, &other).is_err());
        assert!(run_gate(&l, &State { pinned_sha256: Some(sha.clone()), ..State::default() }, &UNPINNED, &sha).is_ok(), "a matching pin needs no confirmation");
        assert!(
            run_gate(&l, &State { pinned_sha256: Some("00".repeat(32)), confirmed_sha256: Some(sha.clone()), ..State::default() }, &UNPINNED, &sha).is_err(),
            "a pin that does not match always blocks"
        );
        let c = installer_command(&l, Path::new("/rt/files/bin/wine"));
        assert_eq!(c.get_program(), "/rt/files/bin/wine");
        assert_eq!(c.get_args().collect::<Vec<_>>(), [l.installer().as_os_str()]);
        assert_eq!(c.get_current_dir(), Some(l.downloads().as_path()));
        let env: Vec<_> = c.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()).unwrap_or_default())).collect();
        assert!(env.contains(&("WINEPREFIX".into(), l.prefix().display().to_string())));
        assert!(env.iter().any(|(k, v)| k == "WINEDLLOVERRIDES" && v.contains("winemenubuilder.exe=d")), "no desktop entries outside the base folder");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_finished_installation_is_merged_into_the_configuration_without_replacing_a_working_one() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("merge");
        prepare(&l).unwrap();
        assert_eq!(adoption_for(&l, &State::default()), None);
        let exe = l.prefix().join("drive_c/Program Files (x86)/Funcom/The Secret World").join(CLIENT_EXE);
        std::fs::create_dir_all(exe.parent().unwrap().join("RDB")).unwrap();
        std::fs::write(exe.parent().unwrap().join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        std::fs::write(&exe, b"MZ").unwrap();
        std::fs::write(l.prefix().join("user.reg"), b"x").unwrap();
        let a = adoption_for(&l, &State::default()).unwrap();
        assert_eq!((a.client_exe.as_path(), a.prefix.as_deref()), (exe.as_path(), Some(l.prefix().as_path())));
        let mut cfg = crate::config::Config::default();
        merge_config(&mut cfg, &a).unwrap();
        assert_eq!((cfg.client_exe.as_deref(), cfg.prefix.as_deref()), (Some(exe.as_path()), Some(l.prefix().as_path())));
        // a configuration that already points to a working client is protected
        let working = root.join("working.exe");
        std::fs::write(&working, b"MZ").unwrap();
        let mut cfg2 = crate::config::Config { client_exe: Some(working.clone()), prefix: Some(root.clone()), ..Default::default() };
        assert!(matches!(merge_config(&mut cfg2, &a), Err(InstallError::Blocked(m)) if m.contains("already points")));
        assert_eq!((cfg2.client_exe, cfg2.prefix), (Some(working), Some(root.clone())), "nothing changed");
        // a stale entry (file gone) may be replaced
        let mut cfg3 = crate::config::Config { client_exe: Some(root.join("gone.exe")), ..Default::default() };
        merge_config(&mut cfg3, &a).unwrap();
        assert_eq!(cfg3.client_exe.as_deref(), Some(exe.as_path()));
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_installer_that_leaves_only_the_patcher_offers_the_patcher_step_until_the_client_exists() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("patcher");
        prepare(&l).unwrap();
        let dir = game_path(&l);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("ClientPatcher.exe"), b"MZ").unwrap();
        let a = assess(&l, &State::default(), None);
        assert_eq!((a.next, a.stage.clone()), (Next::RunPatcher, Stage::PatcherReady { patcher: dir.join("ClientPatcher.exe") }));
        let Stage::PatcherReady { patcher } = a.stage else { unreachable!() };
        let c = patcher_command(&l, Path::new("/rt/files/bin/wine"), &patcher);
        assert_eq!((c.get_current_dir(), c.get_args().collect::<Vec<_>>()), (Some(dir.as_path()), vec![patcher.as_os_str()]));
        assert!(c.get_envs().any(|(k, v)| k == "WINEPREFIX" && v == Some(l.prefix().as_os_str())));
        std::fs::write(dir.join(CLIENT_EXE), b"MZ").unwrap();
        assert_eq!(assess(&l, &State::default(), None).next, Next::RunPatcher, "a client without its data is not finished");
        std::fs::create_dir_all(dir.join("RDB")).unwrap();
        std::fs::write(dir.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        assert_eq!(assess(&l, &State::default(), None).next, Next::Play, "the finished game wins over the patcher");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_patcher_stays_startable_while_the_game_data_may_be_incomplete() {
        let d = std::env::temp_dir().join(format!("tsw-patcher-stage-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let client = d.join("TheSecretWorldDX11.exe");
        std::fs::write(&client, b"MZ").unwrap();
        assert_eq!(patcher_to_run(&Stage::Installed { client: client.clone() }), None, "no patcher beside the client");
        std::fs::write(d.join(PATCHER_EXE), b"MZ").unwrap();
        assert_eq!(patcher_to_run(&Stage::Installed { client }), Some(d.join(PATCHER_EXE)));
        assert_eq!(patcher_to_run(&Stage::PatcherReady { patcher: d.join(PATCHER_EXE) }), Some(d.join(PATCHER_EXE)));
        assert_eq!(patcher_to_run(&Stage::Empty), None);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_layout_is_prefix_centred_and_an_earlier_flatpak_data_layout_is_only_recognised() {
        let home = Path::new("/home/someone");
        let base = default_base(Some(home)).unwrap();
        assert_eq!(base, home.join("Games").join("The Secret World"), "the same default for the host and the Flatpak");
        let l = Layout::new(&base, Some(home)).unwrap();
        assert_eq!(l.prefix(), base.join("prefix"));
        assert_eq!((l.runtime(), l.downloads(), l.cache(), l.logs()), (base.join("runtime"), base.join("downloads"), base.join("cache"), base.join("logs")));
        assert_eq!(l.native_cache(), base.join("cache/native"));
        assert_eq!(game_path(&l), base.join("prefix/drive_c/Program Files (x86)/Funcom/The Secret World"), "the game is directly inside the prefix");
        assert!(!is_legacy_base(&base));
        assert!(is_legacy_base(Path::new("/home/someone/.var/app/org.tswlauncher.Launcher/data/TheSecretWorld")));
        assert!(State::parse("base = /x/y\ngame_dir = /old/link\n").is_ok(), "the key of the dropped external-folder version is ignored, not an error");
    }

    #[test]
    fn the_rdb_import_takes_only_what_the_user_names_never_overwrites_and_never_changes_the_source() {
        let _exec = crate::test_exec_lock();
        let (root, l) = layout("rdb");
        std::fs::create_dir_all(l.prefix()).unwrap();
        std::fs::write(l.prefix().join("user.reg"), b"x").unwrap();
        let src = root.join("their-rdb");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("00.rdbdata"), b"AAAA").unwrap();
        std::fs::write(src.join("RDBHashIndex.bin"), b"INDEX").unwrap();
        let before: Vec<_> = ["00.rdbdata", "RDBHashIndex.bin"].iter().map(|n| (std::fs::read(src.join(n)).unwrap(), std::fs::metadata(src.join(n)).unwrap().modified().unwrap())).collect();
        let dest = game_path(&l).join("RDB");
        // refused inputs
        assert!(import_rdb(&l, Path::new("relative"), false, true).is_err());
        assert!(import_rdb(&l, &root.join("missing"), false, true).is_err());
        assert!(import_rdb(&l, &l.base.join("inside"), false, true).is_err(), "a source inside the managed folder");
        let link = root.join("link");
        std::os::unix::fs::symlink(&src, &link).unwrap();
        assert!(import_rdb(&l, &link, false, true).is_err(), "a symbolic link as the source");
        let sub = root.join("withdir");
        std::fs::create_dir_all(sub.join("nested")).unwrap();
        assert!(import_rdb(&l, &sub, false, true).is_err(), "only plain files");
        // /tmp may not support reflinks: then nothing is copied without the explicit permission
        let first = import_rdb(&l, &src, false, false);
        match first {
            Ok(r) => assert!(r.reflinked && r.copied.len() == 2, "{r:?}"),
            Err(InstallError::Failed(m)) => {
                assert!(m.contains("cannot be cloned") && m.contains("--allow-copy"), "{m}");
                assert!(!dest.join("00.rdbdata").exists() && !dest.join("00.rdbdata.import-part").exists(), "no partial file");
                let r = import_rdb(&l, &src, false, true).unwrap();
                assert!(!r.reflinked && r.copied.len() == 2 && r.bytes == 9, "{r:?}");
            }
            Err(e) => panic!("{e}"),
        }
        assert_eq!(std::fs::read(dest.join("00.rdbdata")).unwrap(), b"AAAA");
        // a second run leaves existing files alone; only the explicit confirmation replaces them
        std::fs::write(dest.join("00.rdbdata"), b"MINE").unwrap();
        let again = import_rdb(&l, &src, false, true).unwrap();
        assert_eq!((again.copied.len(), again.skipped_existing.len()), (0, 2));
        assert_eq!(std::fs::read(dest.join("00.rdbdata")).unwrap(), b"MINE", "never overwritten without confirmation");
        let forced = import_rdb(&l, &src, true, true).unwrap();
        assert_eq!(forced.copied.len(), 2);
        assert_eq!(std::fs::read(dest.join("00.rdbdata")).unwrap(), b"AAAA");
        let after: Vec<_> = ["00.rdbdata", "RDBHashIndex.bin"].iter().map(|n| (std::fs::read(src.join(n)).unwrap(), std::fs::metadata(src.join(n)).unwrap().modified().unwrap())).collect();
        assert_eq!(before, after, "the source is unchanged (content and modification time)");
        let _ = std::fs::remove_dir_all(&root);
    }
}

// ---- what Funcom's own programs say (the installation is only "ready" when the original patcher confirms it) ---------------------------------------

/// What can be observed about the original programs and the patcher's verdict. Gathered by a background watcher, never on the UI thread.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Evidence {
    /// The latest run's verdict, already checked for freshness ([`fresh_verdict`]).
    pub verdict: crate::patchlog::Verdict,
    pub patcher_running: bool,
    pub installer_running: bool,
    /// The patcher's log has not been written for [`CLOSE_QUIET`] (an extra safety condition for closing it, never the reason).
    pub log_quiet: bool,
    /// A look at the running patcher (thread and bytes written), for [`RunWatch`].
    pub sample: Option<RunSample>,
}

/// One look at the running patcher.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct RunSample {
    /// The patcher has the Wine thread `audio_client_ma`: it exists once the patcher plays its "finished" sound (measured: after the last big write of a download, and
    /// at once after the summary of a check that finds nothing to fetch). It does not exist without a sound server.
    pub audio: bool,
    pub wchar: Option<u64>,
}

/// The Wine thread that appears when the patcher announces that it is done.
pub const READY_THREAD: &str = "audio_client_ma";
/// A write rate (bytes per second) that is a download, not the patcher's small bookkeeping (measured: about 20 MiB/s while downloading, below 0.1 MiB/s afterwards).
pub const BULK_RATE: f64 = 2.0 * 1048576.0;
/// How many quiet observations after the ready thread are needed.
pub const READY_QUIET_OBSERVATIONS: u32 = 2;

/// What was observed of ONE run of the original patcher, to decide when it exits whether it ended after finishing its work. Only a consistent picture
/// counts: the patcher announced that it is done AND it did not write big amounts any more afterwards. A "ready" thread followed by more downloading
/// (for example caused by something else) is contradicted for good; no ready thread (no sound server, an interrupted run) never counts.
#[derive(Debug, Clone, Default)]
pub struct RunWatch {
    last: Option<(std::time::Instant, u64)>,
    ready: bool,
    contradicted: bool,
    quiet_after_ready: u32,
}

impl RunWatch {
    pub fn observe(&mut self, now: std::time::Instant, sample: RunSample) {
        let bulk = match (self.last, sample.wchar) {
            (Some((t, before)), Some(now_bytes)) => now_bytes.saturating_sub(before) as f64 / now.duration_since(t).as_secs_f64().max(0.001) >= BULK_RATE,
            _ => false,
        };
        if let Some(bytes) = sample.wchar {
            self.last = Some((now, bytes));
        }
        if self.contradicted {
            return;
        }
        if self.ready {
            if bulk {
                // downloading goes on after the "ready" thread: it did not mean "done"
                (self.ready, self.contradicted, self.quiet_after_ready) = (false, true, 0);
            } else if sample.wchar.is_some() {
                self.quiet_after_ready += 1;
            }
        } else if sample.audio {
            // the interval in which the thread appears still holds the end of the last big write: not held against it
            self.ready = true;
        }
    }

    /// The run ended after the patcher had announced that it is done and had written nothing big for at least [`READY_QUIET_OBSERVATIONS`] observations.
    pub fn finished_cleanly(&self) -> bool {
        self.ready && !self.contradicted && self.quiet_after_ready >= READY_QUIET_OBSERVATIONS
    }
}

/// How long the patcher's log must have been quiet before the finished patcher may be asked to close (in addition to Funcom's confirmation).
pub const CLOSE_QUIET: std::time::Duration = std::time::Duration::from_secs(5);

/// What the big button may do while files of the game exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Funcom confirmed it (or the user already adopted this installation): the installation is ready.
    Open,
    /// One of Funcom's programs is running and has not confirmed completion: wait, never interrupt it.
    Working,
    /// Files exist, nothing is running and nothing confirmed completion (interrupted or unknown): the original patcher has to check (and repair).
    Unverified,
}

/// A verdict from before the running patcher started describes an earlier run (the patcher truncates its log when it starts), so it does not count.
pub fn fresh_verdict(verdict: crate::patchlog::Verdict, log_modified: Option<std::time::SystemTime>, patcher_started: Option<std::time::SystemTime>) -> crate::patchlog::Verdict {
    use std::time::Duration;
    match (patcher_started, log_modified) {
        (Some(started), Some(modified)) if modified + Duration::from_secs(2) < started => crate::patchlog::Verdict::Unknown,
        (Some(_), None) => crate::patchlog::Verdict::Unknown,
        _ => verdict,
    }
}

/// Look at the log and the processes. `state` and `layout` say where the game is. Read-only; a few `stat`s, one bounded file read and a `/proc` scan.
pub fn gather_evidence(layout: &Layout, state: &State) -> Evidence {
    let game = find_installation(layout, state).and_then(|f| f.client.parent().map(Path::to_path_buf)).unwrap_or_else(|| game_path(layout));
    let (verdict, modified) = crate::patchlog::read(&game.join("Patcher.log"));
    let patchers = crate::launch::find_processes(&[PATCHER_EXE], &layout.base);
    let installers = crate::launch::find_processes(&["TheSecretWorldInstaller.exe", "TheSecretWorldInstaller.tmp"], &layout.base);
    let started = patchers.iter().filter_map(|p| p.started).max();
    let log_quiet = modified.is_some_and(|m| m.elapsed().is_ok_and(|age| age >= CLOSE_QUIET));
    let sample = patchers.first().map(|p| {
        let (audio, wchar) = crate::launch::probe_process(p.pid, READY_THREAD);
        RunSample { audio, wchar }
    });
    Evidence { verdict: fresh_verdict(verdict, modified, started), patcher_running: !patchers.is_empty(), installer_running: !installers.is_empty(), log_quiet, sample }
}

/// May the original patcher be asked to close? ONLY when ALL of these hold: this window started it, Funcom's own comparison of THIS run confirmed that
/// nothing is left to fetch (`Old = New roothash`, every counter 0), it is still running, no installer runs, its log has been quiet, and no attempt was made
/// for this run yet. Never after a download (that run's verdict stays "downloading"), never because of CPU, file sizes or a time limit.
pub fn should_close(ev: &Evidence, started_here: bool, attempted: bool) -> bool {
    started_here && !attempted && ev.verdict == crate::patchlog::Verdict::Complete && ev.patcher_running && !ev.installer_running && ev.log_quiet
}

/// The command that asks the original patcher's windows to close, the way a click on the window's close button does: Wine's `taskkill` WITHOUT `/F`
/// (a close message, never a forced end), for this one program name, in this launcher's own prefix (its own Wine server). Run it inside the same sandbox as
/// the patcher.
pub fn close_patcher_command(layout: &Layout, wine: &Path) -> Command {
    let mut c = Command::new(wine);
    c.args(["taskkill", "/im", PATCHER_EXE]).env("WINEPREFIX", layout.prefix()).env("WINEDLLOVERRIDES", "mscoree=d;mshtml=d;winemenubuilder.exe=d").env("WINEDEBUG", "-all");
    c
}

/// How asking the patcher to close ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Closed {
    /// The patcher is gone (it closed itself after the request, or was not running any more).
    Closed,
    /// The request was sent but the patcher is still there after the wait: it stays open, the user closes it.
    StillOpen,
    /// The request could not be sent.
    Failed(String),
}

/// Ask the patcher to close and wait up to `wait` for it to be gone. Nothing is ever forced: no `/F`, no signal, no kill.
pub fn close_patcher(layout: &Layout, wine: &Path, wait: std::time::Duration) -> Closed {
    let running = || !crate::launch::find_processes(&[PATCHER_EXE], &layout.base).is_empty();
    if !running() {
        return Closed::Closed;
    }
    match close_patcher_command(layout, wine).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null()).status() {
        Err(e) => return Closed::Failed(e.to_string()),
        Ok(s) if !s.success() && running() => return Closed::Failed(format!("exit status {s}")),
        Ok(_) => {}
    }
    let until = std::time::Instant::now() + wait;
    loop {
        if !running() {
            return Closed::Closed;
        }
        if std::time::Instant::now() >= until {
            return Closed::StillOpen;
        }
        std::thread::sleep(std::time::Duration::from_millis(250));
    }
}

/// The one decision both the window and the command line use. Only while files of the game exist (`next == Play`) does Funcom's confirmation matter;
/// every other stage is not about completion. The window passes what its watcher observed (`run_finished`); the command line cannot observe a patcher run
/// and always passes `false`, so it never claims a completion that only an observed run could show.
pub fn gate_for(next: Next, adopted: bool, ev: &Evidence, run_finished: bool) -> Gate {
    if next == Next::Play {
        gate(adopted, ev, run_finished)
    } else {
        Gate::Open
    }
}

/// What the patcher's last run said, in words (for the command line).
pub fn verdict_words(v: crate::patchlog::Verdict) -> &'static str {
    use crate::patchlog::Verdict;
    match v {
        Verdict::Unknown => "no readable patcher log, or no run marker in it",
        Verdict::Working => "the last run started but did not report its summary",
        Verdict::Downloading => "the last run reported data to fetch",
        Verdict::Complete => "the last run reported that nothing is left to fetch",
    }
}

/// The decision. `adopted`: the launcher configuration already names a working client (an installation the user chose or an earlier, finished one);
/// that is the user's own decision and is never second-guessed.
pub fn gate(adopted: bool, ev: &Evidence, run_finished: bool) -> Gate {
    // `run_finished`: the last patcher run ended after it announced that it is done and wrote nothing big any more ([`RunWatch`]); the patcher is gone now
    if adopted || ev.verdict == crate::patchlog::Verdict::Complete || (run_finished && !ev.patcher_running && !ev.installer_running) {
        Gate::Open
    } else if ev.patcher_running || ev.installer_running {
        Gate::Working
    } else {
        Gate::Unverified
    }
}

#[cfg(test)]
mod evidence_tests {
    use super::*;
    use crate::patchlog::Verdict;
    use std::time::{Duration, SystemTime};

    fn ev(verdict: Verdict, patcher: bool, installer: bool) -> Evidence {
        Evidence { verdict, patcher_running: patcher, installer_running: installer, log_quiet: true, sample: None }
    }

    /// A watched run: one observation per second; `bulk` writes 20 MiB in that second, otherwise 70 KiB; `audio` says whether the ready thread exists.
    struct Sim {
        w: RunWatch,
        now: std::time::Instant,
        bytes: u64,
    }

    impl Sim {
        fn new() -> Sim {
            let mut s = Sim { w: RunWatch::default(), now: std::time::Instant::now(), bytes: 1_000_000 };
            s.w.observe(s.now, RunSample { audio: false, wchar: Some(s.bytes) });
            s
        }
        fn tick(&mut self, bulk: bool, audio: bool) {
            self.now += std::time::Duration::from_secs(1);
            self.bytes += if bulk { 20 * 1048576 } else { 70 * 1024 };
            self.w.observe(self.now, RunSample { audio, wchar: Some(self.bytes) });
        }
        fn ticks(&mut self, n: u32, bulk: bool, audio: bool) {
            for _ in 0..n {
                self.tick(bulk, audio);
            }
        }
    }

    #[test]
    fn a_run_that_announced_it_is_done_and_wrote_nothing_big_afterwards_counts_as_finished() {
        let mut run = Sim::new();
        run.ticks(60, true, false); // the download
        assert!(!run.w.finished_cleanly());
        run.tick(true, true); // the interval in which the thread appears still holds the end of the last big write
        assert!(!run.w.finished_cleanly(), "not yet: quiet is not proven");
        run.tick(false, true);
        assert!(!run.w.finished_cleanly(), "one quiet observation is not enough");
        run.tick(false, true);
        assert!(run.w.finished_cleanly(), "two quiet observations after the ready thread");
        run.ticks(30, false, true);
        assert!(run.w.finished_cleanly(), "and it stays so while the window is open");
    }

    #[test]
    fn a_ready_thread_in_the_middle_of_a_download_is_contradicted_by_the_writing_that_goes_on() {
        let mut run = Sim::new();
        run.ticks(20, true, false);
        run.tick(true, true); // the thread appears (caused by something else) while the download runs
        run.tick(true, true); // ... and big writes continue
        assert!(!run.w.finished_cleanly());
        run.ticks(50, false, true); // even if it later looks quiet and the thread stays, this run is contradicted for good
        assert!(!run.w.finished_cleanly());
    }

    #[test]
    fn no_ready_thread_means_not_finished_whether_the_run_was_interrupted_or_there_is_no_sound_server() {
        let mut aborted = Sim::new();
        aborted.ticks(25, true, false);
        assert!(!aborted.w.finished_cleanly(), "closed in the middle of the download");
        let mut silent = Sim::new();
        silent.ticks(60, true, false);
        silent.ticks(30, false, false); // finished, but no sound server: nothing announces it
        assert!(!silent.w.finished_cleanly(), "the safe default: not confirmed");
    }

    #[test]
    fn unreadable_write_counters_never_confirm_anything() {
        let mut run = Sim::new();
        run.ticks(10, true, false);
        run.now += std::time::Duration::from_secs(1);
        run.w.observe(run.now, RunSample { audio: true, wchar: None });
        for _ in 0..5 {
            run.now += std::time::Duration::from_secs(1);
            run.w.observe(run.now, RunSample { audio: true, wchar: None });
        }
        assert!(!run.w.finished_cleanly(), "quiet cannot be shown without the counter");
    }

    #[test]
    fn a_finished_run_opens_the_gate_only_after_the_patcher_is_gone() {
        let done = ev(Verdict::Downloading, true, false);
        assert_eq!(gate(false, &done, true), Gate::Working, "the window is still open: wait, never interrupt");
        let gone = ev(Verdict::Downloading, false, false);
        assert_eq!(gate(false, &gone, true), Gate::Open, "closed after it announced that it is done");
        assert_eq!(gate(false, &gone, false), Gate::Unverified, "closed without that: the patcher has to check");
        assert_eq!(gate(false, &ev(Verdict::Downloading, false, true), true), Gate::Working, "an installer is running");
    }

    #[test]
    fn the_patcher_is_only_asked_to_close_when_funcom_confirmed_this_run_and_this_window_started_it() {
        let done = ev(Verdict::Complete, true, false);
        assert!(should_close(&done, true, false));
        assert!(!should_close(&done, false, false), "a patcher this window did not start is never closed");
        assert!(!should_close(&done, true, true), "one attempt per run");
        assert!(!should_close(&Evidence { log_quiet: false, ..done }, true, false), "the log is still being written");
        assert!(!should_close(&Evidence { patcher_running: false, ..done }, true, false), "nothing to close");
        assert!(!should_close(&Evidence { installer_running: true, ..done }, true, false), "an installer is working");
        for verdict in [Verdict::Unknown, Verdict::Working, Verdict::Downloading] {
            assert!(!should_close(&ev(verdict, true, false), true, false), "{verdict:?}: not confirmed, possibly downloading");
        }
    }

    #[test]
    fn closing_sends_a_plain_close_request_for_the_patcher_in_this_prefix_and_never_forces() {
        let layout = Layout { base: PathBuf::from("/games/tsw") };
        let c = close_patcher_command(&layout, Path::new("/rt/bin/wine"));
        assert_eq!(c.get_program(), "/rt/bin/wine");
        let args: Vec<_> = c.get_args().map(|a| a.to_string_lossy().into_owned()).collect();
        assert_eq!(args, ["taskkill", "/im", "ClientPatcher.exe"], "no /F, no /PID, no wildcard");
        let env: std::collections::HashMap<_, _> = c.get_envs().map(|(k, v)| (k.to_string_lossy().into_owned(), v.map(|v| v.to_string_lossy().into_owned()))).collect();
        assert_eq!(env["WINEPREFIX"].as_deref(), Some("/games/tsw/prefix"));
    }

    /// A stand-in for the patcher (`sleep` under the patcher's name, working below the base) and for Wine (a script that records its arguments).
    fn fake_patcher_world(tag: &str, wine_body: &str) -> (Layout, PathBuf, std::process::Child, PathBuf) {
        let base = std::env::temp_dir().join(format!("tsw-close-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let game = game_path(&Layout { base: base.clone() });
        std::fs::create_dir_all(&game).unwrap();
        let exe = crate::sleep_stand_in(&game, PATCHER_EXE);
        let mut child = Command::new(&exe).arg("60").current_dir(&game).spawn().unwrap();
        crate::wait_until_listed(PATCHER_EXE, &base, &mut child);
        let wine = base.join("wine");
        std::fs::write(&wine, format!("#!/bin/sh\necho \"$@ prefix=$WINEPREFIX\" > '{}'\n{wine_body}\n", base.join("wine-args.txt").display())).unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&wine, std::fs::Permissions::from_mode(0o755)).unwrap();
        (Layout { base: base.clone() }, wine, child, base)
    }

    #[test]
    fn a_patcher_that_closes_after_the_request_is_reported_closed_and_one_that_does_not_stays_open() {
        use std::time::Duration;
        let _exec = crate::test_exec_lock();
        // Wine closes the patcher
        let (layout, wine, mut patcher, base) = fake_patcher_world("ok", "true");
        std::fs::write(&wine, format!("#!/bin/sh\necho \"$@ prefix=$WINEPREFIX\" > '{}'\nkill {}\n", base.join("wine-args.txt").display(), patcher.id())).unwrap();
        assert_eq!(close_patcher(&layout, &wine, Duration::from_secs(5)), Closed::Closed);
        let args = std::fs::read_to_string(base.join("wine-args.txt")).unwrap();
        assert!(args.starts_with("taskkill /im ClientPatcher.exe prefix=") && args.contains("/prefix"), "{args}");
        let _ = patcher.wait();
        let _ = std::fs::remove_dir_all(&base);
        // Wine accepts the request but the patcher ignores it: it stays open, nothing is forced
        let (layout, wine, mut patcher, base) = fake_patcher_world("stay", "exit 0");
        assert_eq!(close_patcher(&layout, &wine, Duration::from_millis(900)), Closed::StillOpen);
        assert!(patcher.try_wait().unwrap().is_none(), "the patcher was not touched");
        // Wine cannot send the request
        std::fs::write(&wine, "#!/bin/sh\nexit 3\n").unwrap();
        assert!(matches!(close_patcher(&layout, &wine, Duration::from_millis(500)), Closed::Failed(_)));
        assert!(matches!(close_patcher(&layout, &base.join("no-such-wine"), Duration::from_millis(500)), Closed::Failed(_)));
        assert!(patcher.try_wait().unwrap().is_none(), "still untouched");
        patcher.kill().unwrap();
        let _ = patcher.wait();
        // nothing running any more: nothing to do
        assert_eq!(close_patcher(&layout, &wine, Duration::from_millis(300)), Closed::Closed);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn only_a_confirmed_verdict_or_the_users_own_adoption_opens_the_gate_and_a_running_program_is_never_interrupted() {
        for verdict in [Verdict::Unknown, Verdict::Working, Verdict::Downloading] {
            assert_eq!(gate(false, &ev(verdict, false, false), false), Gate::Unverified, "{verdict:?}: files exist but nothing confirmed it");
            assert_eq!(gate(false, &ev(verdict, true, false), false), Gate::Working, "{verdict:?}: the patcher is running");
            assert_eq!(gate(false, &ev(verdict, false, true), false), Gate::Working, "{verdict:?}: the installer is running");
            assert_eq!(gate(true, &ev(verdict, true, false), false), Gate::Open, "an adopted installation is the user's decision");
        }
        assert_eq!(gate(false, &ev(Verdict::Complete, false, false), false), Gate::Open);
        assert_eq!(gate(false, &ev(Verdict::Complete, true, false), false), Gate::Open, "the patcher is open but idle and has confirmed: ready, nothing is closed");
    }

    #[test]
    fn a_verdict_older_than_the_running_patcher_describes_an_earlier_run() {
        let now = SystemTime::now();
        let earlier = now - Duration::from_secs(60);
        assert_eq!(fresh_verdict(Verdict::Complete, Some(earlier), Some(now)), Verdict::Unknown, "log from before the start of this run");
        assert_eq!(fresh_verdict(Verdict::Complete, Some(now), Some(now - Duration::from_secs(5))), Verdict::Complete);
        assert_eq!(fresh_verdict(Verdict::Complete, Some(now - Duration::from_secs(1)), Some(now)), Verdict::Complete, "a little clock slack");
        assert_eq!(fresh_verdict(Verdict::Complete, None, Some(now)), Verdict::Unknown, "running patcher, no log file at all");
        assert_eq!(fresh_verdict(Verdict::Complete, Some(earlier), None), Verdict::Complete, "no running patcher: the last run's log stands");
        assert_eq!(fresh_verdict(Verdict::Unknown, None, None), Verdict::Unknown);
    }

    #[test]
    fn evidence_is_read_from_the_log_of_the_installed_game_and_defaults_to_nothing_known() {
        // Process observers share the lock with stand-in producers: an exiting foreign patcher can lose its /proc cwd during a scan.
        let _exec = crate::test_exec_lock();
        let dir = std::env::temp_dir().join(format!("tsw-evidence-{}", std::process::id()));
        let layout = Layout { base: dir.clone() };
        let game = game_path(&layout);
        std::fs::create_dir_all(game.join("RDB")).unwrap();
        assert_eq!(gather_evidence(&layout, &State::default()), Evidence::default());
        std::fs::write(game.join("TheSecretWorldDX11.exe"), b"x").unwrap();
        std::fs::write(game.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        let log = "[2026-11-01 17:11:30Z #0] [ID:0] [Main] Patcher exe hash is ab\n[x] [LocalRDBSource] New resources: 0 0.00 MB\n[x] [LocalRDBSource] Changed resources: 0 0.00 MB\n[x] [LocalRDBSource] Changed source hash resources: 0 0.00 MB\n[x] [LocalRDBSource] Deleted resources: 0 0.00 MB\n[x] [LocalRDBSource] Old roothash: aa\n[x] [LocalRDBSource] New roothash: aa\n[x] [RDBManager] Downloading new: 0 0.00 MB\n[x] [RDBManager] Downloading changed: 0 0.00 MB\n[x] [RDBManager] Downloading patches: 0 0.00 MB\n";
        std::fs::write(game.join("Patcher.log"), log).unwrap();
        let got = gather_evidence(&layout, &State::default());
        assert_eq!((got.verdict, got.patcher_running, got.installer_running), (Verdict::Complete, false, false));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
