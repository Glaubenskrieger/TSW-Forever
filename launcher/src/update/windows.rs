//! The Windows launcher update: download the signed installer, check it, start it with the user's consent, wait for its REAL result, and recover.
//!
//! **EXPERIMENTAL and switched off** until the owner's update key is compiled in. The design follows what the Windows spike observed on a real machine
//! (`scripts/windows-update-spike/HANDOVER.md`, section 11); the decision logic here is pure or driven through small traits, so every step is tested on Linux too;
//! only the real process start and the file sharing mode are Windows code (with Windows-only tests).
//!
//! What this module is careful about (the installer runs ELEVATED, from a folder the user can write to):
//! * **No file exchange between check and use.** The downloaded file is opened first with a sharing mode that allows others to read but NOT to write or delete it
//!   (`FILE_SHARE_READ`; opening fails if someone holds it for writing), the digest is computed THROUGH that open handle, and the handle stays open until the
//!   installer process has ENDED (observed: the installer runs fine from a locked file; every write, delete and rename was refused, also while the permission dialog was open).
//! * **The file that is started is the file that was verified:** the download must be an ordinary file (a symbolic link, another reparse point or a folder in its place is
//!   refused: it is opened with `FILE_FLAG_OPEN_REPARSE_POINT` and its attributes are checked), and the installer is started from the REAL path of the held handle
//!   (`GetFinalPathNameByHandleW`, every link on the way resolved), not from the path the download was given. A link on the way could otherwise be pointed somewhere else
//!   after the check (the link itself is not what the lock holds); the real folders above a held file cannot be renamed or replaced while it is held.
//! * **Nothing unverified is started:** exact size and SHA-256 of the signed metadata (see `update::verify_installer_file` for the same rule on a path).
//! * **A fresh, empty folder per attempt** below `%LOCALAPPDATA%\TSW-Forever\updates` (never reused, never a link); it is removed again after any end the launcher survives.
//! * **Direct start, no shell:** the installer is started with `CreateProcess` and plain arguments only (`winnative::is_plain_argument`). Its stub is not elevated
//!   (`asInvoker`): Inno Setup asks Windows for the permission itself, so the user sees Windows' own dialog and the launcher never handles credentials. The process
//!   HANDLE is kept, and its exit code is the result: 0 = installed, 2 = cancelled before anything was installed (what a declined permission dialog looks like), any
//!   other code = the installer failed. No process names, no time-outs.
//! * **The installer may close the launcher** (`/CLOSEAPPLICATIONS`) during a successful installation. Then nobody reads the result: the installer starts the new
//!   launcher by itself (`/RELAUNCH=1`, started by its NOT elevated stage = the original user, not elevated; never use a `runas` start, that relaunches elevated).
//!   [`apply`] therefore returns only while the launcher is still running: exit code 0 then means "installed, the launcher may leave"; anything else is shown as
//!   a failure that changes nothing in the launcher. The window must accept the close request while the installer runs (the GUI does).
//! * **Recovery:** the previous version's installer is signed in the same metadata and installed on request through the very same steps.

use super::{Installer, UpdateError};
use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Arguments of the update installer (Inno Setup): silent progress window, no message boxes, no restart, close the running launcher, and start it again afterwards
/// (`/RELAUNCH=1` is read by the installer script and starts the program as the original, not elevated user; a normal silent installation does not start it).
pub const INSTALLER_ARGS: &[&str] = &["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS", "/RELAUNCH=1"];
/// A check at most this often (seconds).
pub const CHECK_INTERVAL_SECS: u64 = 6 * 3600;
/// Leftover update folders older than this are removed at the next start (seconds).
const LEFTOVER_AGE_SECS: u64 = 24 * 3600;

/// What the update is doing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    Downloading,
    Verifying,
    /// The installer is about to be started (the process does not exist yet).
    Starting,
    /// The installer PROCESS exists and is being waited for (reported only after the start succeeded): from here on it may ask the launcher to close.
    Installing,
}

/// Why an update (or a restore) did not happen. The launcher is unchanged in every case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Failure {
    /// The download failed (offline, server error, too large).
    Network(String),
    /// The file is not the signed one (wrong size or digest) and was deleted.
    Rejected(UpdateError),
    /// A folder or file could not be prepared (or a folder was not safe).
    Storage(String),
    /// The installer process could not be started at all (blocked by security software, a file problem, ...): the reason.
    NotStarted(String),
    /// The installer ended with exit code 2: cancelled before anything was installed, which is what a declined Windows permission dialog looks like. Nothing was changed.
    Declined,
    /// The installer ended with another non-zero exit code (the code): it did not complete.
    InstallerFailed(i32),
    /// The installer was started, but its exit code could not be read: the reason.
    ResultUnknown(String),
}

/// The window's view of the Windows update.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    /// A newer signed version exists (and whether the previous one can be restored).
    Available,
    Working(Step),
    Failed(Failure),
    /// The installer ended successfully while the launcher was still running (it was not closed): the launcher leaves; the new one was started by the installer.
    Started,
}

/// The two things the real system provides.
pub trait Download {
    /// Download `url` into the new file `dest` (at most `max_bytes`).
    fn download(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String>;
}

/// An installer process that was started: its end is its result.
pub trait RunningInstaller {
    /// Wait until the process has ended and return its exit code (blocking, no time-out: a person may need minutes to answer Windows' dialog).
    fn wait(self: Box<Self>) -> Result<i32, String>;
}

pub trait Starter {
    /// Start `installer` in `dir` with `args` DIRECTLY (no shell) and keep the process handle. `Err` = the process could not be started.
    fn start(&self, installer: &Path, dir: &Path, args: &[&str]) -> Result<Box<dyn RunningInstaller>, String>;
}

/// What an installer exit code means (Inno Setup: 0 success; 2 = the user cancelled before the installation began, as when the permission dialog is declined, observed on
/// Windows 11; any other value = it did not complete).
pub fn classify_exit(code: i32) -> Result<(), Failure> {
    match code {
        0 => Ok(()),
        2 => Err(Failure::Declined),
        other => Err(Failure::InstallerFailed(other)),
    }
}

/// A file name that is safe to create below the update folder and to start: plain characters, `.exe`, no path.
pub fn plain_installer_name(name: &str) -> bool {
    name.len() <= 100 && name.ends_with(".exe") && !name.starts_with('.') && !name.contains("..") && name.bytes().all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
}

/// `FILE_FLAG_OPEN_REPARSE_POINT`, `FILE_ATTRIBUTE_DIRECTORY` and `FILE_ATTRIBUTE_REPARSE_POINT`.
#[cfg(windows)]
const OPEN_REPARSE_POINT: u32 = 0x0020_0000;
#[cfg(any(windows, test))]
const ATTRIBUTE_DIRECTORY: u32 = 0x10;
#[cfg(any(windows, test))]
const ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

/// Is an object with these Windows file attributes an ordinary file, one that is not a folder and not a link of any kind (symbolic link, junction, mount point, cloud
/// placeholder or another reparse point)? Everything else is refused as a download.
#[cfg(any(windows, test))]
fn is_ordinary_file(attributes: u32) -> bool {
    attributes & (ATTRIBUTE_REPARSE_POINT | ATTRIBUTE_DIRECTORY) == 0
}

/// Open the downloaded file so that nobody else can write or delete it while we hold it, and make sure it IS an ordinary file. On Windows: `FILE_SHARE_READ` (opening FAILS
/// if another process has the file open for writing) and `FILE_FLAG_OPEN_REPARSE_POINT`: if the last part of the path is a link (a symbolic link or any other reparse point)
/// the link itself is opened, not what it points to, and it is refused here, as is a folder; elsewhere a plain open (the property does not exist there, and the update is
/// Windows-only).
pub fn open_locked(path: &Path) -> std::io::Result<fs::File> {
    let mut o = fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        o.share_mode(1).custom_flags(OPEN_REPARSE_POINT);
    }
    let file = o.open(path)?;
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        // the attributes of the object we really hold (not of a path that may change)
        if !is_ordinary_file(file.metadata()?.file_attributes()) {
            return Err(std::io::Error::new(std::io::ErrorKind::InvalidInput, "the download is a link or a folder, not an ordinary file"));
        }
    }
    Ok(file)
}

/// Where the open file REALLY is: the path with every link on the way resolved (`GetFinalPathNameByHandleW`), in its plain `C:\...` form when it fits. This is the path
/// the installer is started from, so that nothing that happens to a link after the check can make Windows start another file. Elsewhere (no links of this kind matter, the
/// update is Windows-only) the path that was given.
pub fn resolved_path(file: &fs::File, given: &Path) -> std::io::Result<PathBuf> {
    #[cfg(windows)]
    {
        use std::ffi::c_void;
        use std::os::windows::ffi::OsStringExt;
        use std::os::windows::io::AsRawHandle;
        #[link(name = "kernel32")]
        extern "system" {
            fn GetFinalPathNameByHandleW(file: *mut c_void, path: *mut u16, len: u32, flags: u32) -> u32;
        }
        let _ = given;
        let handle = file.as_raw_handle();
        let mut buf = vec![0u16; 1024];
        loop {
            // SAFETY: `handle` is a valid handle borrowed from `file` for the whole call; `buf` is writable for `buf.len()` UTF-16 units; flags 0 = DOS volume name, normalized.
            let n = unsafe { GetFinalPathNameByHandleW(handle, buf.as_mut_ptr(), buf.len() as u32, 0) } as usize;
            if n == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if n < buf.len() {
                buf.truncate(n);
                break;
            }
            buf.resize(n + 1, 0); // the call said how much it needs (including the end mark)
        }
        Ok(PathBuf::from(std::ffi::OsString::from_wide(&plain_form(&buf))))
    }
    #[cfg(not(windows))]
    {
        let _ = file;
        Ok(given.to_path_buf())
    }
}

/// `\\?\C:\x` -> `C:\x` and `\\?\UNC\server\share\x` -> `\\server\share\x`, unless the result would be too long for programs that do not know the long form (then it stays).
#[cfg(any(windows, test))]
fn plain_form(path: &[u16]) -> Vec<u16> {
    const MAX_PLAIN: usize = 240;
    let text = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
    let (verbatim, unc) = (text("\\\\?\\"), text("\\\\?\\UNC\\"));
    if path.starts_with(&unc) && path.len() - unc.len() + 2 < MAX_PLAIN {
        let mut plain = text("\\\\");
        plain.extend_from_slice(&path[unc.len()..]);
        return plain;
    }
    let rest = path.strip_prefix(verbatim.as_slice());
    match rest {
        // only a drive path (`C:\...`) has a plain form
        Some(r) if r.len() >= 3 && r.len() < MAX_PLAIN && r[1] == u16::from(b':') && r[2] == u16::from(b'\\') && (r[0] < 0x80 && (r[0] as u8).is_ascii_alphabetic()) => r.to_vec(),
        _ => path.to_vec(),
    }
}

/// Hash the content of an OPEN file (the handle that is kept) and compare with the signed size and digest.
pub fn verify_open_file(file: &mut fs::File, expected: &Installer) -> Result<(), UpdateError> {
    let mut h = tsw_patch::sha256::Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    let mut size = 0u64;
    loop {
        let n = file.read(&mut buf).map_err(|e| UpdateError::BadDigest(format!("cannot read the download: {e}")))?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > expected.size {
            return Err(UpdateError::SizeMismatch);
        }
        h.update(&buf[..n]);
    }
    if size != expected.size {
        return Err(UpdateError::SizeMismatch);
    }
    if tsw_patch::sha256::hex(&h.finalize()) != expected.sha256 {
        return Err(UpdateError::HashMismatch);
    }
    Ok(())
}

fn storage(what: &str, e: std::io::Error) -> Failure {
    Failure::Storage(format!("{what}: {e}"))
}

/// The update folder, created if needed; never a link.
fn prepare_updates_dir(dir: &Path) -> Result<(), Failure> {
    if let Ok(m) = fs::symlink_metadata(dir) {
        if m.file_type().is_symlink() || !m.is_dir() {
            return Err(Failure::Storage(format!("{} is not an ordinary folder", dir.display())));
        }
    }
    fs::create_dir_all(dir).map_err(|e| storage("cannot create the update folder", e))
}

/// A NEW empty folder for this attempt (`create_dir` fails if the name exists, so nothing is ever reused).
fn new_attempt_dir(updates: &Path, unique: u64) -> Result<PathBuf, Failure> {
    for n in 0..100u32 {
        let d = updates.join(format!("{unique}-{n}"));
        match fs::create_dir(&d) {
            Ok(()) => return Ok(d),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(storage("cannot create a folder for the update", e)),
        }
    }
    Err(Failure::Storage("no free folder name for the update".into()))
}

/// Download, verify (through the held handle), start the installer directly, keep the file locked and wait for the installer's EXIT CODE. `Ok(())` = the installer ended
/// with 0 while this launcher is still running (the installer did not close it): the launcher should leave now, the new one was started by the installer. If the installer
/// closes the launcher during a successful installation this function never returns (the process is gone), which is the normal successful case.
/// `unique` makes the attempt folder name (for example the process id and the time). `progress` is told the step; the attempt folder is removed again after any end that
/// the launcher survives. Blocking: run it on a worker thread.
pub fn apply(download: &dyn Download, starter: &dyn Starter, installer: &Installer, updates_dir: &Path, unique: u64, progress: &mut dyn FnMut(Step)) -> Result<(), Failure> {
    if !plain_installer_name(&installer.name) {
        return Err(Failure::Storage(format!("unusable installer file name: {}", installer.name)));
    }
    prepare_updates_dir(updates_dir)?;
    let attempt = new_attempt_dir(updates_dir, unique)?;
    let result = run_attempt(download, starter, installer, &attempt, progress);
    // the file lock is released by now (the handle lives only inside `run_attempt`), so the folder can go
    let _ = fs::remove_dir_all(&attempt);
    result
}

fn run_attempt(download: &dyn Download, starter: &dyn Starter, installer: &Installer, attempt: &Path, progress: &mut dyn FnMut(Step)) -> Result<(), Failure> {
    let file_path = attempt.join(&installer.name);
    progress(Step::Downloading);
    download.download(&installer.url, &file_path, installer.size).map_err(Failure::Network)?;
    progress(Step::Verifying);
    if fs::symlink_metadata(&file_path).map(|m| !m.is_file()).unwrap_or(true) {
        return Err(Failure::Storage("the download is not an ordinary file".into()));
    }
    // from here on the file is held: nobody else can change it until the installer process has ENDED (see below)
    let mut held = open_locked(&file_path).map_err(|e| storage("cannot lock the download", e))?;
    if let Err(e) = verify_open_file(&mut held, installer) {
        drop(held);
        let _ = fs::remove_file(&file_path);
        return Err(Failure::Rejected(e));
    }
    // the file that was verified and is held, wherever it physically is: every link on the way is resolved, so that the path that is started cannot be redirected by changing a link later
    let start_path = resolved_path(&held, &file_path).map_err(|e| storage("cannot resolve the locked download", e))?;
    let start_dir = start_path.parent().map(Path::to_path_buf).unwrap_or_else(|| attempt.to_path_buf());
    progress(Step::Starting);
    let running = starter.start(&start_path, &start_dir, INSTALLER_ARGS).map_err(Failure::NotStarted)?;
    progress(Step::Installing);
    // the result of the installation is the exit code of the process we started; while we wait, the lock stays
    let waited = running.wait();
    drop(held);
    match waited {
        Ok(code) => classify_exit(code),
        Err(e) => Err(Failure::ResultUnknown(e)),
    }
}

/// Remove update folders that were left behind (an installer that was still running at the end of an earlier attempt, a crash). Folders younger than a day stay.
pub fn clean_leftovers(updates_dir: &Path, now_secs: u64) {
    let Ok(rd) = fs::read_dir(updates_dir) else { return };
    for e in rd.flatten() {
        let Ok(m) = e.metadata() else { continue };
        if m.file_type().is_symlink() || !m.is_dir() {
            continue;
        }
        let age = m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| now_secs.saturating_sub(d.as_secs()));
        if age.is_some_and(|a| a > LEFTOVER_AGE_SECS) {
            let _ = fs::remove_dir_all(e.path());
        }
    }
}

/// Is a new check due? (`last` = the time of the last one.) A clock that went backwards counts as due.
pub fn check_due(now_secs: u64, last: Option<u64>) -> bool {
    match last {
        Some(l) if l <= now_secs => now_secs - l >= CHECK_INTERVAL_SECS,
        _ => true,
    }
}

/// The recorded time of the last check: one decimal number in a small text file; anything else counts as "never".
pub fn parse_last_check(text: &str) -> Option<u64> {
    let t = text.trim();
    (!t.is_empty() && t.len() <= 12 && t.bytes().all(|b| b.is_ascii_digit())).then(|| t.parse().ok()).flatten()
}

// ---- the real system -------------------------------------------------------------------------------------------------------

/// `curl` as the downloader: https only (also for redirects), few redirects, timeouts, a hard size limit, writes only to the given new file.
pub struct CurlDownload {
    pub curl: PathBuf,
}

impl Download for CurlDownload {
    fn download(&self, url: &str, dest: &Path, max_bytes: u64) -> Result<(), String> {
        if !url.starts_with("https://") || url.chars().any(|c| !(c.is_ascii_alphanumeric() || matches!(c, ':' | '/' | '.' | '-' | '_'))) {
            return Err("invalid update address".into());
        }
        let mut cmd = std::process::Command::new(&self.curl);
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
            "10",
            "--max-time",
            "900",
            "--max-filesize",
        ])
        .arg(max_bytes.to_string())
        .arg("--output")
        .arg(dest)
        .arg("--")
        .arg(url)
        .stdin(std::process::Stdio::null());
        let out = cmd.output().map_err(|e| format!("cannot start curl: {e}"))?;
        if out.status.success() {
            Ok(())
        } else {
            Err(String::from_utf8_lossy(&out.stderr).trim().chars().take(300).collect())
        }
    }
}

/// The real start: `CreateProcess` directly (no shell, no `cmd`), plain arguments only, no inherited standard handles. Inno Setup's stub is not elevated, so this works
/// without error 740 and Windows shows its own permission dialog from inside the installer.
pub struct DirectStarter;

/// A started process whose handle is kept until `wait` has returned (the real start exists on Windows only).
#[cfg(windows)]
struct ChildInstaller(std::process::Child);

#[cfg(windows)]
impl RunningInstaller for ChildInstaller {
    fn wait(mut self: Box<Self>) -> Result<i32, String> {
        let status = self.0.wait().map_err(|e| e.to_string())?;
        status.code().ok_or_else(|| "the installer ended without an exit code".to_string())
    }
}

impl Starter for DirectStarter {
    #[cfg(windows)]
    fn start(&self, installer: &Path, dir: &Path, args: &[&str]) -> Result<Box<dyn RunningInstaller>, String> {
        if !args.iter().all(|a| crate::winnative::is_plain_argument(a)) {
            return Err("an installer argument is not safe to pass".into());
        }
        let child = std::process::Command::new(installer)
            .args(args)
            .current_dir(dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .map_err(|e| e.to_string())?;
        Ok(Box::new(ChildInstaller(child)))
    }

    #[cfg(not(windows))]
    fn start(&self, _installer: &Path, _dir: &Path, _args: &[&str]) -> Result<Box<dyn RunningInstaller>, String> {
        Err("the launcher update through an installer exists on Windows only".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::rc::Rc;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-winupd-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    const BYTES: &[u8] = b"fixture installer bytes - not a program";

    fn installer(bytes: &[u8]) -> Installer {
        Installer {
            name: "tsw-forever-0.3.5-windows-setup.exe".into(),
            url: "https://github.com/Glaubenskrieger/TSW-Forever/releases/download/v0.3.5/tsw-forever-0.3.5-windows-setup.exe".into(),
            size: bytes.len() as u64,
            sha256: tsw_patch::sha256::sha256_hex(bytes),
        }
    }

    type DuringWait = Rc<dyn Fn(&Path)>;

    /// Download, start and running installer in one: records what happened, answers with the chosen exit code, and can run a check while the "installer" is running.
    struct Fake {
        content: Result<Vec<u8>, String>,
        start_result: Result<(), String>,
        exit: Result<i32, String>,
        log: Rc<RefCell<Vec<String>>>,
        during_wait: Option<DuringWait>,
        /// the installer path each `start` was given
        paths: Rc<RefCell<Vec<PathBuf>>>,
    }

    impl Fake {
        fn exits(code: i32) -> Fake {
            Fake { content: Ok(BYTES.to_vec()), start_result: Ok(()), exit: Ok(code), log: Rc::new(RefCell::new(vec![])), during_wait: None, paths: Rc::new(RefCell::new(vec![])) }
        }
        fn ok() -> Fake {
            Fake::exits(0)
        }
    }

    struct Running {
        exit: Result<i32, String>,
        log: Rc<RefCell<Vec<String>>>,
        path: PathBuf,
        during_wait: Option<DuringWait>,
    }

    impl RunningInstaller for Running {
        fn wait(self: Box<Self>) -> Result<i32, String> {
            self.log.borrow_mut().push("wait".into());
            if let Some(check) = &self.during_wait {
                check(&self.path);
            }
            self.log.borrow_mut().push("ended".into());
            self.exit.clone()
        }
    }

    impl Download for Fake {
        fn download(&self, url: &str, dest: &Path, max: u64) -> Result<(), String> {
            self.log.borrow_mut().push(format!("download {url} max={max}"));
            fs::write(dest, self.content.clone()?).map_err(|e| e.to_string())
        }
    }

    impl Starter for Fake {
        fn start(&self, installer: &Path, dir: &Path, args: &[&str]) -> Result<Box<dyn RunningInstaller>, String> {
            assert!(installer.starts_with(dir) && installer.is_file(), "the installer exists below its own folder");
            self.log.borrow_mut().push(format!("start {} {}", installer.file_name().unwrap().to_string_lossy(), args.join(" ")));
            self.log.borrow_mut().push(format!("folder {}", dir.file_name().unwrap().to_string_lossy()));
            self.paths.borrow_mut().push(installer.to_path_buf());
            self.start_result.clone()?;
            Ok(Box::new(Running { exit: self.exit.clone(), log: self.log.clone(), path: installer.to_path_buf(), during_wait: self.during_wait.clone() }))
        }
    }

    fn run(f: &Fake, i: &Installer, dir: &Path) -> (Result<(), Failure>, Vec<Step>) {
        let mut steps = vec![];
        let r = apply(f, f, i, dir, 7, &mut |s| steps.push(s));
        (r, steps)
    }

    fn nothing_left(updates: &Path) -> bool {
        fs::read_dir(updates).map(|mut r| r.next().is_none()).unwrap_or(true)
    }

    #[test]
    fn a_verified_installer_is_started_directly_with_plain_arguments_and_its_exit_code_is_the_result() {
        let d = tmp("ok");
        let f = Fake::ok();
        let (r, steps) = run(&f, &installer(BYTES), &d.join("updates"));
        assert_eq!(r, Ok(()));
        assert_eq!(steps, vec![Step::Downloading, Step::Verifying, Step::Starting, Step::Installing]);
        let log = f.log.borrow();
        assert!(log[0].starts_with("download https://github.com/Glaubenskrieger/TSW-Forever/releases/download/v0.3.5/") && log[0].ends_with(&format!("max={}", BYTES.len())));
        assert_eq!(log[1], "start tsw-forever-0.3.5-windows-setup.exe /SILENT /SUPPRESSMSGBOXES /NORESTART /CLOSEAPPLICATIONS /RELAUNCH=1");
        assert_eq!(&log[3..], ["wait", "ended"], "start, then the exit code is awaited: no polling, no process names, no time-out");
        for a in INSTALLER_ARGS {
            assert!(crate::winnative::is_plain_argument(a), "{a}");
        }
        assert!(nothing_left(&d.join("updates")), "the attempt folder goes once the installer has ended and the launcher is still here");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_exit_code_decides_zero_is_installed_two_is_declined_everything_else_failed() {
        assert_eq!(classify_exit(0), Ok(()));
        assert_eq!(classify_exit(2), Err(Failure::Declined));
        for code in [1, 3, 4, 5, 6, 7, 8, 255, -1, i32::MAX, i32::MIN, 1_073_741_510] {
            assert_eq!(classify_exit(code), Err(Failure::InstallerFailed(code)), "{code}");
        }
        let d = tmp("codes");
        for (code, want) in [(0, Ok(())), (2, Err(Failure::Declined)), (1, Err(Failure::InstallerFailed(1))), (5, Err(Failure::InstallerFailed(5)))] {
            let f = Fake::exits(code);
            let (r, steps) = run(&f, &installer(BYTES), &d.join("updates"));
            assert_eq!(r, want, "exit code {code}");
            assert_eq!(steps.last(), Some(&Step::Installing), "exit code {code}: the installer ran");
            assert!(nothing_left(&d.join("updates")), "exit code {code}: nothing is left behind");
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_that_is_not_the_signed_one_is_deleted_and_never_started() {
        for bad in [vec![0u8; BYTES.len()], BYTES[..10].to_vec(), [BYTES, b"x"].concat(), vec![]] {
            let d = tmp("bad");
            let f = Fake { content: Ok(bad.clone()), ..Fake::ok() };
            let (r, steps) = run(&f, &installer(BYTES), &d.join("updates"));
            assert!(matches!(r, Err(Failure::Rejected(UpdateError::HashMismatch | UpdateError::SizeMismatch))), "{bad:?}: {r:?}");
            assert_eq!(steps, vec![Step::Downloading, Step::Verifying], "never reaches the start");
            assert!(!f.log.borrow().iter().any(|l| l.starts_with("start")));
            assert!(nothing_left(&d.join("updates")), "the attempt folder is removed again");
            let _ = fs::remove_dir_all(&d);
        }
    }

    #[test]
    fn every_way_the_update_can_end_without_an_installation_is_a_distinct_ordinary_result_that_leaves_nothing_behind() {
        let d = tmp("fail");
        let u = d.join("updates");
        // offline
        let net = Fake { content: Err("Could not resolve host".into()), ..Fake::ok() };
        let (r, steps) = run(&net, &installer(BYTES), &u);
        assert_eq!((r, steps), (Err(Failure::Network("Could not resolve host".into())), vec![Step::Downloading]));
        // the process cannot be started (security software, a file problem)
        let blocked = Fake { start_result: Err("Access is denied. (os error 5)".into()), ..Fake::ok() };
        let (r, steps) = run(&blocked, &installer(BYTES), &u);
        assert_eq!(r, Err(Failure::NotStarted("Access is denied. (os error 5)".into())));
        assert_eq!(steps.last(), Some(&Step::Starting), "a start that failed never reports an installer that runs");
        assert!(!blocked.log.borrow().iter().any(|l| l == "wait"), "nothing to wait for when the start failed");
        // the exit code cannot be read
        let lost = Fake { exit: Err("handle is invalid".into()), ..Fake::ok() };
        assert_eq!(run(&lost, &installer(BYTES), &u).0, Err(Failure::ResultUnknown("handle is invalid".into())));
        // a declined permission dialog: exit code 2, observed on Windows 11
        assert_eq!(run(&Fake::exits(2), &installer(BYTES), &u).0, Err(Failure::Declined));
        assert!(nothing_left(&u), "nothing is left behind by any of them");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn unusable_file_names_and_unsafe_folders_are_refused_before_anything_happens() {
        let d = tmp("names");
        for name in ["", "a b.exe", "..\\evil.exe", "../evil.exe", "x.exe ", "setup", "setup.msi", ".hidden.exe", "a..b.exe", "a\\b.exe", "C:evil.exe", "x;y.exe", &format!("{}.exe", "a".repeat(100))]
        {
            assert!(!plain_installer_name(name), "{name:?}");
            let f = Fake::ok();
            let i = Installer { name: name.into(), ..installer(BYTES) };
            assert!(matches!(run(&f, &i, &d.join("u")).0, Err(Failure::Storage(_))), "{name:?}");
            assert!(f.log.borrow().is_empty(), "{name:?}: nothing was downloaded");
        }
        assert!(plain_installer_name("tsw-forever-0.3.5-windows-setup.exe"));
        // a file where the folder should be, and (on Unix) a link in its place
        fs::write(d.join("file"), "x").unwrap();
        assert!(matches!(run(&Fake::ok(), &installer(BYTES), &d.join("file")).0, Err(Failure::Storage(_))));
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&d, d.join("link")).unwrap();
            assert!(matches!(run(&Fake::ok(), &installer(BYTES), &d.join("link")).0, Err(Failure::Storage(_))), "a link is never an update folder");
        }
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn every_attempt_gets_its_own_new_folder_and_an_existing_one_is_never_reused() {
        let d = tmp("fresh");
        let u = d.join("updates");
        fs::create_dir_all(u.join("7-0")).unwrap();
        fs::write(u.join("7-0").join("planted.exe"), "planted").unwrap();
        let f = Fake::ok();
        assert_eq!(run(&f, &installer(BYTES), &u).0, Ok(()));
        assert_eq!(fs::read(u.join("7-0").join("planted.exe")).unwrap(), b"planted", "the planted folder is untouched");
        assert!(f.log.borrow().iter().any(|l| l == "folder 7-1"), "a new folder was used: {:?}", f.log.borrow());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_digest_is_computed_through_the_open_handle_and_the_limits_are_exact() {
        let d = tmp("handle");
        let p = d.join("f.exe");
        fs::write(&p, BYTES).unwrap();
        let mut h = open_locked(&p).unwrap();
        assert_eq!(verify_open_file(&mut h, &installer(BYTES)), Ok(()));
        // on Windows the open handle holds the lock (FILE_SHARE_READ): release it before the file is rewritten (found on real Windows, where this write failed with error 32)
        drop(h);
        // a file that grows beyond the signed size is stopped while reading
        fs::write(&p, vec![b'a'; 1_000_000]).unwrap();
        let mut h = open_locked(&p).unwrap();
        assert_eq!(verify_open_file(&mut h, &installer(BYTES)), Err(UpdateError::SizeMismatch));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn leftovers_older_than_a_day_are_removed_and_fresh_ones_stay() {
        let d = tmp("left");
        fs::create_dir_all(d.join("old")).unwrap();
        fs::create_dir_all(d.join("new")).unwrap();
        fs::write(d.join("file.txt"), "keep").unwrap();
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
        clean_leftovers(&d, now);
        assert!(d.join("old").is_dir() && d.join("new").is_dir(), "nothing is older than a day yet");
        clean_leftovers(&d, now + 3 * 86_400);
        assert!(!d.join("old").exists() && !d.join("new").exists(), "three days later both are gone");
        assert!(d.join("file.txt").is_file(), "only folders are touched");
        clean_leftovers(&d.join("missing"), now);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_check_runs_at_most_every_six_hours_and_a_bad_record_means_never() {
        assert!(check_due(1_000_000, None));
        assert!(!check_due(1_000_000, Some(1_000_000 - 3_600)));
        assert!(check_due(1_000_000, Some(1_000_000 - CHECK_INTERVAL_SECS)));
        assert!(check_due(1_000_000, Some(2_000_000)), "a clock that went backwards");
        assert_eq!(parse_last_check("1796119200\n"), Some(1_796_119_200));
        for bad in ["", "abc", "-5", "12.5", "1796119200000000", "12 34"] {
            assert_eq!(parse_last_check(bad), None, "{bad:?}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn the_curl_downloader_passes_only_a_https_address_a_size_limit_and_one_output_file() {
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("curl");
        let args = d.join("args.txt");
        let stub = d.join("curl");
        fs::write(
            &stub,
            format!(
                "#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\nout=\"\"; prev=\"\"; for a in \"$@\"; do [ \"$prev\" = --output ] && out=\"$a\"; prev=\"$a\"; done\nprintf 'data' > \"$out\"\n",
                args.display()
            ),
        )
        .unwrap();
        fs::set_permissions(&stub, fs::Permissions::from_mode(0o755)).unwrap();
        let dl = CurlDownload { curl: stub };
        let dest = d.join("setup.exe");
        dl.download("https://github.com/Glaubenskrieger/TSW-Forever/releases/download/v0.3.5/x.exe", &dest, 4096).unwrap();
        assert_eq!(fs::read(&dest).unwrap(), b"data");
        let a = fs::read_to_string(&args).unwrap();
        for need in ["--fail", "--proto\n=https", "--proto-redir\n=https", "--max-redirs\n3", "--max-filesize\n4096", "--output", "--\nhttps://github.com/"] {
            assert!(a.contains(need), "{need:?} in {a}");
        }
        for bad in ["http://github.com/x", "https://github.com/x y", "https://github.com/x?y=1", "file:///etc/passwd", "ftp://x/y", ""] {
            assert_eq!(dl.download(bad, &d.join("z.exe"), 10), Err("invalid update address".into()), "{bad:?}");
        }
        assert!(!d.join("z.exe").exists());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn the_direct_starter_exists_on_windows_only_and_refuses_unsafe_arguments() {
        #[cfg(not(windows))]
        assert!(DirectStarter.start(Path::new("/x/setup.exe"), Path::new("/x"), &["/SILENT"]).is_err());
        #[cfg(windows)]
        for bad in ["/SILENT \"&calc", "a b", "%PATH%", "x;y", ""] {
            assert!(DirectStarter.start(Path::new("C:\\Windows\\System32\\cmd.exe"), Path::new("C:\\Windows\\System32"), &[bad]).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn only_an_ordinary_file_passes_and_every_kind_of_link_or_folder_is_refused() {
        // normal, read-only, hidden, system, archive, temporary, sparse, compressed, encrypted, not-content-indexed: all ordinary files
        for ordinary in [0x80, 0x1, 0x2, 0x4, 0x20, 0x100, 0x200, 0x800, 0x4000, 0x2000, 0x20 | 0x2 | 0x1] {
            assert!(is_ordinary_file(ordinary), "{ordinary:#x}");
        }
        // a reparse point (symbolic link, junction, mount point, cloud placeholder, ...), a folder, and any combination with them
        for refused in [0x400, 0x10, 0x410, 0x400 | 0x20, 0x400 | 0x200, 0x10 | 0x1, 0x400 | 0x80] {
            assert!(!is_ordinary_file(refused), "{refused:#x}");
        }
    }

    #[test]
    fn the_plain_form_of_a_final_path_is_used_only_where_it_exists_and_fits() {
        let w = |s: &str| s.encode_utf16().collect::<Vec<u16>>();
        let back = |v: Vec<u16>| String::from_utf16(&v).unwrap();
        assert_eq!(back(plain_form(&w("\\\\?\\C:\\Users\\a\\x.exe"))), "C:\\Users\\a\\x.exe");
        assert_eq!(back(plain_form(&w("\\\\?\\UNC\\server\\share\\x.exe"))), "\\\\server\\share\\x.exe");
        for stays in ["C:\\already\\plain.exe", "\\\\?\\Volume{1234}\\x.exe", "\\\\?\\GLOBALROOT\\x", "\\\\?\\1:\\x", "\\\\?\\C:x", ""] {
            assert_eq!(back(plain_form(&w(stays))), stays, "{stays}");
        }
        let long = format!("\\\\?\\C:\\{}\\x.exe", "a".repeat(300));
        assert_eq!(back(plain_form(&w(&long))), long, "a path that is too long keeps its long form");
    }

    #[cfg(windows)]
    fn junction(link: &Path, target: &Path) -> bool {
        std::process::Command::new("cmd")
            .args(["/C", "mklink", "/J"])
            .arg(link)
            .arg(target)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    }

    /// An ordinary file opens, resolves to itself in plain form and is the very file that was opened.
    #[cfg(windows)]
    #[test]
    fn an_ordinary_file_resolves_to_itself_in_plain_form() {
        let d = tmp("resolve");
        let p = d.join("f.exe");
        fs::write(&p, BYTES).unwrap();
        let h = open_locked(&p).unwrap();
        let r = resolved_path(&h, &p).unwrap();
        let text = r.to_string_lossy().to_string();
        assert!(!text.starts_with("\\\\?\\"), "{text}");
        assert!(r.is_file());
        assert_eq!(text.to_lowercase(), fs::canonicalize(&p).unwrap().to_string_lossy().trim_start_matches("\\\\?\\").to_lowercase());
        let _ = fs::remove_dir_all(&d);
    }

    /// The path the update folder is reached through may contain a link (a folder junction needs no privilege): the installer is started from the REAL folder, never
    /// through the junction, because a junction can be removed and made again pointing elsewhere while only the real file is held.
    #[cfg(windows)]
    #[test]
    fn the_installer_is_started_from_the_real_folder_and_not_through_a_link_on_the_way() {
        let base = tmp("viajunction");
        let real = base.join("real-folder");
        fs::create_dir_all(real.join("updates")).unwrap();
        let via = base.join("via-junction");
        if !junction(&via, &real) {
            eprintln!("skipped: a folder junction cannot be created here");
            return;
        }
        let f = Fake::ok();
        let (r, _) = run(&f, &installer(BYTES), &via.join("updates"));
        assert_eq!(r, Ok(()));
        let started = f.paths.borrow()[0].to_string_lossy().to_lowercase();
        assert!(started.contains("\\real-folder\\updates\\") && !started.contains("via-junction"), "started from {started}");
        fs::remove_dir(&via).unwrap();
        let _ = fs::remove_dir_all(&base);
    }

    /// A folder, a junction or (when the account may create them) a symbolic link in place of the download is refused: it is neither verified nor started.
    #[cfg(windows)]
    #[test]
    fn a_link_or_a_folder_in_place_of_the_download_is_refused_and_nothing_is_started() {
        let base = tmp("linkdownload");
        let real = base.join("real");
        fs::create_dir_all(&real).unwrap();
        let target_file = real.join("target.exe");
        fs::write(&target_file, BYTES).unwrap();
        // a folder
        assert!(open_locked(&real).is_err(), "a folder is not a download");
        // a junction (no privilege needed)
        let jl = base.join("junction.exe");
        if junction(&jl, &real) {
            assert!(open_locked(&jl).is_err(), "a junction is not a download");
            fs::remove_dir(&jl).unwrap();
        } else {
            eprintln!("skipped: a folder junction cannot be created here");
        }
        // a symbolic link to a REAL file with exactly the signed content: it would pass the digest, but the link could be pointed elsewhere later
        let sl = base.join("symlink.exe");
        match std::os::windows::fs::symlink_file(&target_file, &sl) {
            Ok(()) => {
                assert!(open_locked(&sl).is_err(), "a symbolic link is not a download");
                fs::remove_file(&sl).unwrap();
            }
            Err(e) => eprintln!("skipped: a symbolic link cannot be created without the privilege ({e})"),
        }
        // the whole attempt: a downloader that leaves a junction where the file should be
        struct PlantJunction(PathBuf);
        impl Download for PlantJunction {
            fn download(&self, _url: &str, dest: &Path, _max: u64) -> Result<(), String> {
                if junction(dest, &self.0) {
                    Ok(())
                } else {
                    Err("junction not possible".into())
                }
            }
        }
        let f = Fake::ok();
        let mut steps = vec![];
        let r = apply(&PlantJunction(real.clone()), &f, &installer(BYTES), &base.join("updates"), 7, &mut |s| steps.push(s));
        match r {
            Err(Failure::Network(why)) if why == "junction not possible" => eprintln!("skipped: a folder junction cannot be created here"),
            other => {
                assert!(matches!(other, Err(Failure::Storage(_))), "{other:?}");
                assert!(!f.log.borrow().iter().any(|l| l.starts_with("start")), "nothing was started");
            }
        }
        assert!(real.join("target.exe").is_file(), "what the link pointed to was not touched");
        let _ = fs::remove_dir_all(&base);
    }

    /// Why the real path can be trusted after the check: every real folder above a held file is pinned too (it cannot be renamed, moved or removed).
    #[cfg(windows)]
    #[test]
    fn every_real_folder_above_a_held_file_is_pinned_as_long_as_it_is_held() {
        let d = tmp("pinned");
        let deep = d.join("a").join("b").join("c");
        fs::create_dir_all(&deep).unwrap();
        let p = deep.join("f.exe");
        fs::write(&p, BYTES).unwrap();
        let held = open_locked(&p).unwrap();
        for (name, folder) in [("parent", deep.clone()), ("grandparent", d.join("a").join("b")), ("great-grandparent", d.join("a"))] {
            assert!(fs::rename(&folder, folder.with_extension("away")).is_err(), "{name} cannot be moved away");
            assert!(fs::remove_dir_all(&folder).is_err() && p.is_file(), "{name} cannot be removed");
        }
        drop(held);
        assert!(fs::rename(d.join("a"), d.join("moved")).is_ok(), "after the release the folders can be moved again");
        let _ = fs::remove_dir_all(&d);
    }

    /// The file stays locked for the whole life of the installer process, whatever its exit code. (A lock exists on Windows only.)
    #[cfg(windows)]
    #[test]
    fn the_downloaded_file_is_locked_while_the_installer_runs_and_released_when_it_has_ended() {
        use std::os::windows::fs::OpenOptionsExt;
        for code in [0, 2, 1] {
            let d = tmp("lockrun");
            let u = d.join("updates");
            let outcomes: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(vec![]));
            let seen = outcomes.clone();
            let check: DuringWait = Rc::new(move |p: &Path| {
                let sharing = |e: std::io::Error| e.raw_os_error() == Some(32);
                let mut o = seen.borrow_mut();
                o.push(format!(
                    "write {}",
                    fs::OpenOptions::new().write(true).share_mode(7).open(p).map(|_| "ALLOWED".to_string()).unwrap_or_else(|e| if sharing(e) { "refused".into() } else { "other error".into() })
                ));
                o.push(format!("delete {}", if fs::remove_file(p).is_err() { "refused" } else { "ALLOWED" }));
                o.push(format!("rename {}", if fs::rename(p, p.with_extension("moved")).is_err() { "refused" } else { "ALLOWED" }));
                let other = p.with_extension("other");
                fs::write(&other, b"planted").unwrap();
                o.push(format!("replace {}", if fs::rename(&other, p).is_err() { "refused" } else { "ALLOWED" }));
                o.push(format!("folder {}", if fs::rename(p.parent().unwrap(), p.parent().unwrap().with_extension("away")).is_err() { "refused" } else { "ALLOWED" }));
                assert!(fs::File::open(p).is_ok(), "reading stays possible (the loader maps a locked program)");
            });
            let f = Fake { during_wait: Some(check), ..Fake::exits(code) };
            let (r, _) = run(&f, &installer(BYTES), &u);
            assert_eq!(r, classify_exit(code), "exit code {code}");
            assert_eq!(
                *outcomes.borrow(),
                ["write refused", "delete refused", "rename refused", "replace refused", "folder refused"],
                "exit code {code}: every change was refused while the installer ran"
            );
            assert!(nothing_left(&u), "exit code {code}: once it ended, the lock is gone and the folder could be removed");
            let _ = fs::remove_dir_all(&d);
        }
    }
}

/// Real processes on a real Windows machine: the direct start, the kept handle, the exit codes, and what a child does (not) inherit.
#[cfg(all(test, windows))]
mod process_tests {
    use super::*;

    fn cmd() -> PathBuf {
        PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32").join("cmd.exe")
    }

    fn system32(program: &str) -> PathBuf {
        cmd().with_file_name(program)
    }

    fn dir() -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-winproc-{}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn a_real_process_is_started_directly_and_its_exit_code_is_read_from_the_kept_handle() {
        let d = dir();
        for code in [0, 2, 3, 5, 255] {
            let running = DirectStarter.start(&cmd(), &d, &["/C", "exit", &code.to_string()]).unwrap_or_else(|e| panic!("start: {e}"));
            assert_eq!(running.wait(), Ok(code), "the exit code of the real process");
            assert_eq!(classify_exit(code).is_ok(), code == 0);
        }
        // what the spike saw when the permission dialog was declined (the installer's first stage ended with 2)
        let declined = DirectStarter.start(&cmd(), &d, &["/C", "exit", "2"]).unwrap().wait().unwrap();
        assert_eq!(classify_exit(declined), Err(Failure::Declined));
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn waiting_really_blocks_until_the_process_has_ended() {
        let d = dir();
        let t = std::time::Instant::now();
        // `ping -n 3` takes about two seconds and ends with 0
        let running = DirectStarter.start(&system32("PING.EXE"), &d, &["-n", "3", "127.0.0.1"]).unwrap();
        assert_eq!(running.wait(), Ok(0));
        assert!(t.elapsed() >= std::time::Duration::from_millis(1500), "wait() returned after {:?}, before the process could have ended", t.elapsed());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_program_is_started_from_the_resolved_path_of_its_held_handle() {
        let d = dir();
        let ping = system32("PING.EXE");
        let held = open_locked(&ping).unwrap();
        let real = resolved_path(&held, &ping).unwrap();
        assert!(!real.to_string_lossy().starts_with(r"\\?\") && real.is_file(), "{}", real.display());
        let running = DirectStarter.start(&real, &d, &["-n", "1", "127.0.0.1"]).unwrap();
        assert_eq!(running.wait(), Ok(0), "the resolved path starts the program");
        drop(held);
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_program_that_cannot_be_started_is_an_error_and_nothing_runs() {
        let d = dir();
        assert!(DirectStarter.start(&d.join("does-not-exist.exe"), &d, &["/SILENT"]).is_err());
        // a text file is not a program (Windows refuses with an error, never a shell prompt)
        let text = d.join("not-a-program.exe");
        fs::write(&text, b"this is not an executable").unwrap();
        assert!(DirectStarter.start(&text, &d, &["/SILENT"]).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_started_installer_does_not_inherit_the_lock_handle_so_the_lock_ends_with_the_launchers_own_handle() {
        let d = dir();
        let file = d.join("locked.exe");
        fs::write(&file, b"x").unwrap();
        let held = open_locked(&file).unwrap();
        // a slow child that is still running when the handle is dropped
        let running = DirectStarter.start(&system32("PING.EXE"), &d, &["-n", "4", "127.0.0.1"]).unwrap();
        assert!(fs::OpenOptions::new().write(true).open(&file).is_err(), "locked while the child runs");
        drop(held);
        assert!(fs::OpenOptions::new().write(true).open(&file).is_ok(), "the child did not inherit the handle: the lock is gone with ours");
        assert_eq!(running.wait(), Ok(0));
        let _ = fs::remove_dir_all(&d);
    }
}

/// The claims about the file lock, checked on a real Windows machine (these tests only exist there): what `open_locked` really prevents.
#[cfg(all(test, windows))]
mod lock_tests {
    use super::*;
    use std::os::windows::fs::OpenOptionsExt;

    const ERROR_SHARING_VIOLATION: i32 = 32;

    fn dir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-winlock-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    /// A copy of this very test program: a real Windows executable that can be run while it is locked (`--help` is answered by the test harness).
    fn locked_program(d: &Path) -> PathBuf {
        let p = d.join("setup.exe");
        fs::copy(std::env::current_exe().unwrap(), &p).unwrap();
        p
    }

    #[test]
    fn a_locked_file_cannot_be_written_deleted_renamed_or_replaced_but_can_be_read_and_started() {
        let d = dir("held");
        let p = locked_program(&d);
        let held = open_locked(&p).unwrap();
        let violation = |e: std::io::Error| e.raw_os_error() == Some(ERROR_SHARING_VIOLATION);
        // writing, whatever sharing the other side offers
        assert!(violation(fs::OpenOptions::new().write(true).open(&p).unwrap_err()), "write");
        assert!(violation(fs::OpenOptions::new().write(true).share_mode(7).open(&p).unwrap_err()), "write with every sharing mode");
        assert!(violation(fs::OpenOptions::new().append(true).open(&p).unwrap_err()), "append");
        assert!(violation(fs::File::create(&p).unwrap_err()), "truncate");
        // delete, rename, replace
        assert!(fs::remove_file(&p).is_err(), "delete");
        assert!(fs::rename(&p, d.join("other.exe")).is_err(), "rename the file away");
        let other = d.join("evil.exe");
        fs::write(&other, b"MZ evil").unwrap();
        assert!(fs::rename(&other, &p).is_err(), "replace it by renaming another file over it");
        assert!(fs::copy(&other, &p).is_err(), "replace it by copying over it");
        assert!(fs::OpenOptions::new().read(true).access_mode(0x0001_0000).share_mode(7).open(&p).is_err(), "open with DELETE access (delete-on-close)");
        assert_eq!(fs::read(&other).unwrap(), b"MZ evil", "the attacker's file was not used");
        // the folder with the locked file inside: an OBSERVATION for the report, not an assertion (the exact rule for directories is not documented as firmly)
        let folder = fs::rename(&d, d.with_extension("moved"));
        eprintln!("observation: renaming the folder of a locked file: {:?}", folder.as_ref().map_err(|e| e.raw_os_error()));
        if folder.is_ok() {
            fs::rename(d.with_extension("moved"), &d).unwrap();
        }
        // reading and starting are still possible: the loader can map a locked program
        assert!(fs::File::open(&p).is_ok(), "read");
        let out = std::process::Command::new(&p).arg("--help").output().expect("the locked program starts");
        assert!(out.status.success(), "{:?}", out.status);
        drop(held);
        fs::OpenOptions::new().write(true).open(&p).expect("after the lock is released the file can be written again");
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_file_somebody_else_holds_for_writing_cannot_be_locked_so_a_swap_in_progress_stops_the_update() {
        let d = dir("writer");
        let p = locked_program(&d);
        // an attacker (or a still running downloader) keeps a write handle with the most generous sharing
        let writer = fs::OpenOptions::new().write(true).share_mode(7).open(&p).unwrap();
        let e = open_locked(&p).unwrap_err();
        assert_eq!(e.raw_os_error(), Some(ERROR_SHARING_VIOLATION), "locking fails instead of verifying a file that can still change");
        drop(writer);
        assert!(open_locked(&p).is_ok());
        let _ = fs::remove_dir_all(&d);
    }

    #[test]
    fn a_change_made_before_the_lock_is_caught_by_the_digest_through_the_same_handle() {
        let d = dir("swap");
        let p = d.join("setup.exe");
        fs::write(&p, BYTES_FOR_LOCK_TEST).unwrap();
        let signed = Installer { name: "setup.exe".into(), url: String::new(), size: BYTES_FOR_LOCK_TEST.len() as u64, sha256: tsw_patch::sha256::sha256_hex(BYTES_FOR_LOCK_TEST) };
        // the swap happens before the lock: the digest taken through the held handle sees the swapped content
        fs::write(&p, b"MZ swapped before the lock, same length padding............").unwrap();
        let mut held = open_locked(&p).unwrap();
        assert!(verify_open_file(&mut held, &signed).is_err());
        let _ = fs::remove_dir_all(&d);
    }

    const BYTES_FOR_LOCK_TEST: &[u8] = b"MZ the signed bytes of the installer fixture";
}
