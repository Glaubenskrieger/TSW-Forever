//! tsw-launch: proof-of-concept launcher for the original The Secret World client.
//!
//! Implemented: the client's start-up self-checks (`-clienthash`, `-key`), command construction and process start
//! for Windows (native) and Linux (Wine/Proton), with secrets masked in everything that is printed.
//! NOT implemented (see documentation/architecture.md): patching/updating, authentication
//! (`-loginkey`/`-statindex` come from outside and are only passed through), prefix creation, compatibility fixes.

pub mod config;
pub mod diag;
pub mod diaglog;
pub mod discover;
pub mod display;
pub mod doctor;
pub mod dxredist;
pub mod dxsetup;
pub mod fps;
pub mod gamescope;
pub mod install;
pub mod installcmd;
pub mod key;
pub mod launch;
pub mod md5;
pub mod patchcmd;
pub mod patchlog;
pub mod prefix;
pub mod prefs;
pub mod progress;
pub mod project;
pub mod runner;
pub mod runtimedl;
pub mod secrets;
pub mod selfupdate;
pub mod sha256;
pub mod shadercache;
pub mod windowing;
pub mod winnative;

use std::fmt;
use std::path::Path;

/// Everything the client needs from us before it will start, computed from the client executable itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientCredentials {
    /// 32 hex characters: MD5 of the client executable file (`-clienthash`).
    pub client_hash: String,
    /// 16 hex characters: `%08x%08x` of TEA(t, t) (`-key`).
    pub patcher_key: String,
}

#[derive(Debug)]
pub enum LaunchError {
    Io(std::io::Error, String),
    KeyNotFound,
    SelfCheckFailed,
    Usage(String),
}

impl fmt::Display for LaunchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LaunchError::Io(e, what) => write!(f, "{what}: {e}"),
            LaunchError::KeyNotFound => write!(f, "the patcher-key cipher key was not found in this executable (unknown build?); refusing to guess"),
            LaunchError::SelfCheckFailed => write!(f, "internal self-check of the generated key failed"),
            LaunchError::Usage(m) => write!(f, "{m}"),
        }
    }
}

impl std::error::Error for LaunchError {}

/// Read the client executable once; compute the hash and the key for time `now` (Unix seconds).
pub fn credentials_for_exe(exe: &Path, now: u64) -> Result<ClientCredentials, LaunchError> {
    let bytes = std::fs::read(exe).map_err(|e| LaunchError::Io(e, format!("cannot read {}", exe.display())))?;
    credentials_for_bytes(&bytes, now)
}

pub fn credentials_for_bytes(exe: &[u8], now: u64) -> Result<ClientCredentials, LaunchError> {
    let cipher = key::find_cipher_key(exe).ok_or(LaunchError::KeyNotFound)?;
    let patcher_key = key::patcher_key_arg(now, &cipher);
    if !key::client_would_accept(&patcher_key, now, &cipher) {
        return Err(LaunchError::SelfCheckFailed);
    }
    let mut m = md5::Md5::new();
    m.update(exe);
    Ok(ClientCredentials { client_hash: md5::hex(&m.finalize()), patcher_key })
}

/// Options for [`client_arguments`]. `extra` are caller-supplied arguments passed through verbatim
/// (for example `-loginkey`/`-statindex` obtained elsewhere); this crate never creates them.
#[derive(Debug, Clone, Default)]
pub struct ArgOptions {
    pub dx11: bool,
    pub no_console: bool,
    pub extra: Vec<String>,
}

/// The argument list the original patcher passes to the client (subset that the client requires),
/// in the same order as the patcher: `-noconsole`, `-clienthash H`, `-key K`, extra, `-dx9|-dx11`.
pub fn client_arguments(cred: &ClientCredentials, opt: &ArgOptions) -> Vec<String> {
    let mut a = Vec::new();
    if opt.no_console {
        a.push("-noconsole".to_string());
    }
    a.extend(["-clienthash".to_string(), cred.client_hash.clone(), "-key".to_string(), cred.patcher_key.clone()]);
    a.extend(opt.extra.iter().cloned());
    a.push(if opt.dx11 { "-dx11" } else { "-dx9" }.to_string());
    a
}

/// Argument names whose value must never be printed or logged.
const SECRET_FLAGS: &[&str] = &["-loginkey", "-statindex"];

/// A copy of `args` that is safe to print: the value following a secret flag is replaced.
pub fn redacted(args: &[String]) -> Vec<String> {
    let mut out = Vec::with_capacity(args.len());
    let mut mask_next = false;
    for a in args {
        if mask_next {
            out.push("<redacted>".to_string());
            mask_next = false;
        } else {
            mask_next = SECRET_FLAGS.contains(&a.as_str());
            out.push(a.clone());
        }
    }
    out
}

/// A test that writes a script and executes it at once races with EVERY `fork` of a parallel test (`ETXTBSY`: "Text file busy"): a forked child inherits the
/// still-open write handle until its own `exec` closes it, and `exec` of a file that any process holds open for writing fails. Measured: about one start in four
/// fails while other threads start processes. So the lock is held by every test that writes an executable AND by every test that starts a process at all (the
/// forks of the second kind are what hit the first kind); `every_test_that_starts_a_process_holds_the_exec_lock` enforces the second rule.
#[cfg(test)]
pub(crate) fn test_exec_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

/// A stand-in program called `name` that just sleeps: a SYMLINK to `/bin/sleep`, not a copy. `exec` through a symlink runs the real `sleep` with the stand-in's
/// path as its command line, which is all the process lookups read, and no file is ever opened for writing, so it cannot fail with `ETXTBSY`.
#[cfg(all(test, unix))]
pub(crate) fn sleep_stand_in(dir: &std::path::Path, name: &str) -> std::path::PathBuf {
    let exe = dir.join(name);
    let _ = std::fs::remove_file(&exe);
    std::os::unix::fs::symlink("/bin/sleep", &exe).unwrap();
    exe
}

/// A process that `Command::spawn` just started is not yet visible to `launch::find_processes`: `spawn` returns once `exec` has happened, but the kernel
/// publishes the new command line in `/proc/<pid>/cmdline` a moment later (right after `spawn` it is empty, measured in about 85 % of the starts; the
/// window is tens of microseconds, but a loaded or virtual machine can preempt the child inside it for milliseconds). A test that looks for its stand-in
/// process at once depends on that timing. This waits (bounded) until `child` is listed under `name` below `under`, so the test controls the process it
/// observes; if the stand-in has already exited, it says so instead of waiting.
#[cfg(all(test, unix))]
pub(crate) fn wait_until_listed(name: &str, under: &std::path::Path, child: &mut std::process::Child) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    let pid = child.id();
    while !launch::find_processes(&[name], under).iter().any(|r| r.pid == pid) {
        if let Some(status) = child.try_wait().unwrap() {
            panic!("the stand-in process {pid} ({name}) exited by itself ({status}) before the test looked at it");
        }
        assert!(std::time::Instant::now() < deadline, "process {pid} ({name}) did not become visible in /proc within 10 s");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rule behind `test_exec_lock`: a test that starts a process forks, and that fork can make the `exec` of a script another test has just written fail
    /// with `ETXTBSY`. Process observers also share the lock: a concurrently exiting stand-in can lose its /proc cwd during a scan.
    /// So no unit test of this crate may start or observe a process without holding the lock. Needles are assembled so this test does not match itself.
    #[test]
    fn every_test_that_starts_a_process_holds_the_exec_lock() {
        let needles = [
            format!("{}::new", "Command"),
            format!(".{}(", "spawn"),
            format!("{}_masked", "run"),
            format!("{}_for_client", "wait"),
            format!("{}_patcher(", "close"),
            format!("{}_evidence(", "gather"),
            format!("{}_processes(", "find"),
        ];
        let lock = format!("{}_exec_lock", "test");
        let mut offenders = Vec::new();
        for entry in std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/src")).unwrap().flatten() {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "rs") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            let mut rest = text.as_str();
            while let Some(at) = rest.find("#[test]") {
                rest = &rest[at + 7..];
                let Some(open) = rest.find('{') else { break };
                // the body ends where the braces balance again
                let (mut depth, mut end) = (0usize, rest.len());
                for (i, ch) in rest[open..].char_indices() {
                    depth += (ch == '{') as usize;
                    depth -= (ch == '}') as usize;
                    if depth == 0 {
                        end = open + i + 1;
                        break;
                    }
                }
                let (head, body) = (&rest[..open], &rest[open..end]);
                if needles.iter().any(|n| body.contains(n.as_str())) && !body.contains(lock.as_str()) {
                    let name = head.split("fn ").nth(1).and_then(|s| s.split('(').next()).unwrap_or("?").trim().to_string();
                    offenders.push(format!("{}: {name}", path.file_name().unwrap().to_string_lossy()));
                }
                rest = &rest[end..];
            }
        }
        assert!(offenders.is_empty(), "these tests start or observe a process without holding test_exec_lock(): {offenders:?}");
    }

    /// The stand-in program must start reliably while other threads fork all the time (the situation of the parallel test run). A COPY of `sleep` fails in
    /// about one start in four here (measured); the symlink must never fail.
    #[cfg(unix)]
    #[test]
    fn the_sleep_stand_in_starts_reliably_while_other_threads_fork() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let _exec = test_exec_lock();
        let dir = std::env::temp_dir().join(format!("tsw-standin-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let stop = std::sync::Arc::new(AtomicBool::new(false));
        let forkers: Vec<_> = (0..3)
            .map(|_| {
                let stop = stop.clone();
                std::thread::spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        let _ = std::process::Command::new("/bin/true").status();
                    }
                })
            })
            .collect();
        let mut failures = Vec::new();
        for i in 0..300 {
            let exe = sleep_stand_in(&dir, &format!("ClientPatcher-{i}.exe"));
            if let Err(e) = std::process::Command::new(&exe).arg("0.001").current_dir(&dir).status() {
                failures.push(e.to_string());
            }
        }
        stop.store(true, Ordering::Relaxed);
        for f in forkers {
            f.join().unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert!(failures.is_empty(), "{} of 300 stand-in starts failed, e.g. {:?}", failures.len(), failures.first());
    }

    fn sample_exe() -> Vec<u8> {
        let mut buf = b"MZ....Invalid patcher key arg \0\0\0".to_vec();
        for w in [0x5bce568eu32, 0xfb2e8cbc, 0xa324e6d8, 0xf22bf865] {
            buf.extend_from_slice(&w.to_le_bytes());
        }
        buf.extend_from_slice(b"Could not decode patcher key \0");
        buf
    }

    #[test]
    fn credentials_hash_the_whole_file_and_key_is_accepted() {
        let exe = sample_exe();
        let c = credentials_for_bytes(&exe, 1_793_300_058).unwrap();
        assert_eq!(c.client_hash, md5::md5_hex(&exe));
        assert_eq!(c.patcher_key, "e6ecbe480decdc7b");
    }

    #[test]
    fn unknown_executable_is_refused() {
        assert!(matches!(credentials_for_bytes(b"MZ nothing", 1), Err(LaunchError::KeyNotFound)));
    }

    #[test]
    fn argument_order_and_redaction() {
        let c = ClientCredentials { client_hash: "h".repeat(32), patcher_key: "k".repeat(16) };
        let opt = ArgOptions { dx11: true, no_console: true, extra: vec!["-loginkey".into(), "SECRET".into(), "-statindex".into(), "42".into()] };
        let a = client_arguments(&c, &opt);
        assert_eq!(a.first().map(String::as_str), Some("-noconsole"));
        assert_eq!(a.last().map(String::as_str), Some("-dx11"));
        let r = redacted(&a);
        assert!(!r.iter().any(|x| x == "SECRET" || x == "42"));
        assert_eq!(r.iter().filter(|x| *x == "<redacted>").count(), 2);
        assert!(a.iter().any(|x| x == "SECRET"), "the real arguments are untouched");
    }
}
