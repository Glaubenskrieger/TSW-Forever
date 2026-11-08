//! Provides the Wine/Proton runner inside the launcher's own data folder, for machines (a fresh Steam Machine, a Flatpak sandbox) that have none.
//!
//! The runner is ONE pinned release (GE-Proton, [`GE_PROTON`]): a fixed https address and the SHA-256 of that exact archive. Nothing else can be
//! fetched, the user cannot type an address, and the download starts only with explicit consent. The archive gets its final name only after the
//! checksum matched; it is unpacked into a staging folder below `<base>/runtime/` (the one place that is writable and persistent in the
//! Flatpak sandbox and found by [`crate::discover::find_runtimes`]), and the finished folder replaces nothing that already exists.
//! Removing the runner later is deleting that folder; no other data is touched.
//!
//! The pin: size and SHA-256 were measured on 2026-10-31 from the release asset `GE-Proton11-11-x86_64.tar.gz` of
//! <https://github.com/GloriousEggroll/proton-ge-custom/releases/tag/GE-Proton11-11> and agree with the `sha512sum` file and the digest that
//! GitHub shows for the asset (documentation/third-party-components.md). A new runner version is a deliberate code change with a new pin.

use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::Ordering;
use std::time::Duration;

use crate::install::{fetch_verified, Downloader, InstallError};
use crate::progress::{Outcome, Phase, Progress, Update};

/// The one runner the launcher can provide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Runner {
    /// Name of the folder the archive contains and the launcher keeps (`GE-Proton11-11-x86_64`).
    pub dir_name: &'static str,
    /// File name of the archive in the cache.
    pub archive: &'static str,
    pub url: &'static str,
    pub sha256: &'static str,
    pub size: u64,
}

pub const GE_PROTON: Runner = Runner {
    dir_name: "GE-Proton11-11-x86_64",
    archive: "GE-Proton11-11-x86_64.tar.gz",
    url: "https://github.com/GloriousEggroll/proton-ge-custom/releases/download/GE-Proton11-11/GE-Proton11-11-x86_64.tar.gz",
    sha256: "aa85986cada5709aa8bbfa8add7f3bfb925cbe15710c98995898d308ff2ca3c1",
    size: 564_789_552,
};

/// Is the runner already in place (its wine program exists)?
pub fn is_installed(runtimes: &Path, runner: &Runner) -> bool {
    runtimes.join(runner.dir_name).join("files").join("bin").join("wine").is_file()
}

fn failed(what: &str, e: io::Error) -> InstallError {
    InstallError::Failed(format!("{what}: {e}"))
}

/// Entries of the archive that would leave the target folder (absolute path, `..`). Checked before anything is unpacked.
fn unsafe_entry(listing: &str) -> Option<String> {
    listing.lines().find(|l| l.starts_with('/') || l.split('/').any(|c| c == "..")).map(|l| l.to_string())
}

/// Download (resumed, verified) and unpack the runner. `consent` must be true: the caller obtained the user's agreement for exactly [`Runner::url`].
/// Returns the folder of the runner. Cancelling keeps the partial download and removes the staging folder.
pub fn install(runtimes: &Path, runner: &Runner, consent: bool, tar: &Path, dl: &Downloader) -> Result<PathBuf, InstallError> {
    let result = run(runtimes, runner, consent, tar, dl);
    dl.sink.update(Update::Done(match &result {
        Ok(_) => Outcome::Finished("The runner was downloaded, verified and unpacked.".into()),
        Err(e) => Outcome::Failed(e.to_string()),
    }));
    result
}

fn run(runtimes: &Path, runner: &Runner, consent: bool, tar: &Path, dl: &Downloader) -> Result<PathBuf, InstallError> {
    if !consent {
        return Err(InstallError::Blocked(format!("the runner download needs your explicit consent ({} MB from {})", runner.size / 1_000_000, runner.url)));
    }
    if !runner.url.starts_with("https://") || runner.dir_name.contains('/') || runner.dir_name.contains("..") || runner.archive.contains('/') {
        return Err(InstallError::Blocked("the pinned runner entry is not acceptable".into()));
    }
    let target = runtimes.join(runner.dir_name);
    if is_installed(runtimes, runner) {
        return Ok(target);
    }
    if target.exists() {
        return Err(InstallError::NotEmpty(target));
    }
    std::fs::create_dir_all(runtimes).map_err(|e| failed("cannot create the runtimes folder", e))?;
    let cache = runtimes.join(".download");
    std::fs::create_dir_all(&cache).map_err(|e| failed("cannot create the download folder", e))?;
    let (part, archive) = (cache.join(format!("{}.part", runner.archive)), cache.join(runner.archive));
    if !archive.is_file() {
        fetch_verified(dl, runner.url, &part, &archive, runner.sha256, "Downloading the runner")?;
    } else {
        let (hash, _) = crate::sha256::file_sha256(&archive).map_err(|e| failed("cannot read the cached archive", e))?;
        if !hash.eq_ignore_ascii_case(runner.sha256) {
            let _ = std::fs::remove_file(&archive);
            return Err(InstallError::Failed("the cached archive does not match the pinned checksum and was removed; try again".into()));
        }
    }
    dl.sink.update(Update::Progress(Progress::indeterminate(Phase::Installing, "Unpacking the runner")));
    let listing = Command::new(tar).arg("-tzf").arg(&archive).stdin(Stdio::null()).output().map_err(|e| failed(&format!("cannot start {}", tar.display()), e))?;
    if !listing.status.success() {
        return Err(InstallError::Failed(format!("the archive cannot be read: {}", String::from_utf8_lossy(&listing.stderr).trim())));
    }
    let names = String::from_utf8_lossy(&listing.stdout).into_owned();
    if let Some(bad) = unsafe_entry(&names) {
        return Err(InstallError::Failed(format!("the archive contains an unsafe path ({bad}); nothing was unpacked")));
    }
    let top = format!("{}/", runner.dir_name);
    if !names.lines().all(|l| l == runner.dir_name || l.starts_with(&top)) {
        return Err(InstallError::Failed(format!("the archive does not consist of the single folder {}; nothing was unpacked", runner.dir_name)));
    }
    let staging = runtimes.join(format!(".staging-{}", runner.dir_name));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| failed("cannot create the staging folder", e))?;
    let mut child = Command::new(tar)
        .arg("-xzf")
        .arg(&archive)
        .arg("-C")
        .arg(&staging)
        .args(["--no-same-owner", "--no-same-permissions"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| failed(&format!("cannot start {}", tar.display()), e))?;
    let status = loop {
        if dl.cancel.load(Ordering::SeqCst) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = std::fs::remove_dir_all(&staging);
            return Err(InstallError::Cancelled);
        }
        if let Some(s) = child.try_wait().map_err(|e| failed("waiting for tar", e))? {
            break s;
        }
        std::thread::sleep(Duration::from_millis(100));
    };
    if !status.success() {
        let mut err = String::new();
        if let Some(mut s) = child.stderr.take() {
            let _ = io::Read::read_to_string(&mut s, &mut err);
        }
        let _ = std::fs::remove_dir_all(&staging);
        return Err(InstallError::Failed(format!("unpacking failed ({status}): {}", err.trim())));
    }
    let unpacked = staging.join(runner.dir_name);
    if !unpacked.join("files").join("bin").join("wine").is_file() {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(InstallError::Failed("the unpacked runner has no files/bin/wine".into()));
    }
    std::fs::rename(&unpacked, &target).map_err(|e| failed("cannot move the runner into place", e))?;
    let _ = std::fs::remove_dir_all(&staging);
    // the archive is a cache; keep it only if the user wants to reinstall without downloading (it is large)
    let _ = std::fs::remove_file(&archive);
    let _ = std::fs::remove_dir(&cache);
    Ok(target)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use std::sync::Mutex;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-runtimedl-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn script(path: &Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        // the file is executed at once by another thread of the test run: make sure it is closed and visible
        let _ = std::fs::File::open(path).and_then(|f| f.sync_all());
    }

    /// Build a small tar.gz with the real `tar`; returns (archive, sha256).
    fn archive(root: &Path, top: &str, with_wine: bool) -> (PathBuf, String) {
        let src = root.join("src");
        let wine = src.join(top).join("files").join("bin");
        std::fs::create_dir_all(&wine).unwrap();
        if with_wine {
            script(&wine.join("wine"), "exit 0");
        }
        std::fs::write(src.join(top).join("version"), "1 test").unwrap();
        let out = root.join("a.tar.gz");
        let st = Command::new("tar").arg("-czf").arg(&out).arg("-C").arg(&src).arg(top).status().unwrap();
        assert!(st.success());
        let (h, _) = crate::sha256::file_sha256(&out).unwrap();
        (out, h)
    }

    fn fake_curl(dir: &Path, payload: &Path) -> PathBuf {
        let curl = dir.join("curl");
        script(
            &curl,
            &format!(
                r#"case " $* " in *" --head "*) printf 'HTTP/1.1 200 OK\r\nContent-Length: %s\r\n\r\n' "$(wc -c < '{p}')"; exit 0;; esac
out=""; prev=""; for a in "$@"; do if [ "$prev" = "--output" ]; then out="$a"; fi; prev="$a"; done
cat '{p}' > "$out""#,
                p = payload.display()
            ),
        );
        curl
    }

    fn runner(hash: &str) -> Runner {
        Runner { dir_name: "GE-Test", archive: "GE-Test.tar.gz", url: "https://example.invalid/GE-Test.tar.gz", sha256: hash.to_string().leak(), size: 1000 }
    }

    fn run_install(rt: &Path, r: &Runner, consent: bool, curl: &Path) -> (Result<PathBuf, InstallError>, Vec<Update>) {
        let cancel = AtomicBool::new(false);
        let seen = Mutex::new(Vec::<Update>::new());
        let sink = |u: Update| seen.lock().unwrap().push(u);
        let res = install(rt, r, consent, Path::new("tar"), &Downloader { curl, cancel: &cancel, sink: &sink });
        (res, seen.into_inner().unwrap())
    }

    #[test]
    fn the_shipped_pin_is_well_formed() {
        let _exec = crate::test_exec_lock();
        assert!(GE_PROTON.url.starts_with("https://github.com/GloriousEggroll/proton-ge-custom/releases/download/"));
        assert!(crate::sha256::is_hex_digest(GE_PROTON.sha256));
        assert!(GE_PROTON.url.ends_with(GE_PROTON.archive) && GE_PROTON.archive.starts_with(GE_PROTON.dir_name));
        assert!(!GE_PROTON.dir_name.contains('/'));
    }

    #[test]
    fn without_consent_nothing_is_fetched_and_a_verified_archive_is_unpacked_into_the_runtimes_folder() {
        let _exec = crate::test_exec_lock();
        let root = tmp("ok");
        let (arc, hash) = archive(&root, "GE-Test", true);
        let curl = fake_curl(&root, &arc);
        let rt = root.join("data").join("tsw-launch").join("runtimes");
        let r = runner(&hash);
        let (res, seen) = run_install(&rt, &r, false, &curl);
        assert!(matches!(res, Err(InstallError::Blocked(_))), "{res:?}");
        assert!(!rt.exists(), "no consent: not even the folder is created");
        assert!(matches!(seen.last(), Some(Update::Done(Outcome::Failed(_)))));
        let (res, seen) = run_install(&rt, &r, true, &curl);
        let dir = res.unwrap();
        assert_eq!(dir, rt.join("GE-Test"));
        assert!(is_installed(&rt, &r));
        assert!(!rt.join(".download").exists() && !rt.join(".staging-GE-Test").exists(), "cache and staging are cleaned up");
        assert!(matches!(seen.last(), Some(Update::Done(Outcome::Finished(_)))));
        // already there: nothing happens, nothing is replaced
        let (again, _) = run_install(&rt, &r, true, &curl);
        assert_eq!(again.unwrap(), dir);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_wrong_checksum_is_discarded_and_nothing_is_unpacked() {
        let _exec = crate::test_exec_lock();
        let root = tmp("bad");
        let (arc, _) = archive(&root, "GE-Test", true);
        let curl = fake_curl(&root, &arc);
        let rt = root.join("runtimes");
        let (res, _) = run_install(&rt, &runner(&"00".repeat(32)), true, &curl);
        assert!(matches!(&res, Err(InstallError::Failed(m)) if m.contains("checksum mismatch")), "{res:?}");
        assert!(!rt.join("GE-Test").exists() && !rt.join(".download").join("GE-Test.tar.gz").exists());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_archive_with_another_layout_or_without_wine_is_refused() {
        let _exec = crate::test_exec_lock();
        let root = tmp("layout");
        let (arc, hash) = archive(&root, "Other", true);
        let curl = fake_curl(&root, &arc);
        let (res, _) = run_install(&root.join("rt1"), &runner(&hash), true, &curl);
        assert!(matches!(&res, Err(InstallError::Failed(m)) if m.contains("single folder")), "{res:?}");
        assert!(!root.join("rt1").join("GE-Test").exists());
        let root2 = tmp("nowine");
        let (arc2, hash2) = archive(&root2, "GE-Test", false);
        let curl2 = fake_curl(&root2, &arc2);
        let (res2, _) = run_install(&root2.join("rt"), &runner(&hash2), true, &curl2);
        assert!(matches!(&res2, Err(InstallError::Failed(m)) if m.contains("files/bin/wine")), "{res2:?}");
        assert!(!root2.join("rt").join("GE-Test").exists() && !root2.join("rt").join(".staging-GE-Test").exists());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&root2);
    }

    #[test]
    fn an_existing_foreign_folder_is_never_replaced() {
        let _exec = crate::test_exec_lock();
        let root = tmp("exists");
        let (arc, hash) = archive(&root, "GE-Test", true);
        let curl = fake_curl(&root, &arc);
        let rt = root.join("rt");
        std::fs::create_dir_all(rt.join("GE-Test")).unwrap();
        std::fs::write(rt.join("GE-Test").join("mine"), "x").unwrap();
        let (res, _) = run_install(&rt, &runner(&hash), true, &curl);
        assert!(matches!(res, Err(InstallError::NotEmpty(_))));
        assert_eq!(std::fs::read(rt.join("GE-Test").join("mine")).unwrap(), b"x");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn paths_that_leave_the_target_are_detected() {
        let _exec = crate::test_exec_lock();
        assert_eq!(unsafe_entry("GE/a\nGE/b/c"), None);
        assert!(unsafe_entry("GE/a\n/etc/passwd").is_some());
        assert!(unsafe_entry("GE/../x").is_some());
        assert!(unsafe_entry("../x").is_some());
    }
}
