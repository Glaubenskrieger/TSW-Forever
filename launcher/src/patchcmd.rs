//! Client patches in the launcher: glue between the configuration, the detected installation and the `tsw-patch` component.
//!
//! * Disabled until the launcher contains a trusted public key ([`crate::project::PATCH_PUBLIC_KEY`]); then the game starts as before.
//! * The source is the latest GitHub release of the project (or `patch_source`); always https; only the signed manifest decides what happens.
//! * Never blocks the game: no network, a bad signature, a running game or any error is reported and the installed patch state is kept,
//!   so a fully patched installation can be played offline.
//!
//! CLI: `tsw-launch patch status|check|apply|verify`.

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use tsw_patch::apply::{self, Options};
use tsw_patch::fetch::CurlFetch;
use tsw_patch::manifest::Manifest;
use tsw_patch::{sig, PatchError};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// No trusted key / switched off: nothing happens.
    Disabled(String),
    UpToDate {
        installed: u64,
    },
    Available {
        installed: u64,
        latest: u64,
    },
    Applied {
        from: u64,
        to: u64,
        files: usize,
    },
    /// The source cannot be reached; the installed state is untouched and the game can be played.
    Offline(String),
    /// Not now (the game is running, another update runs).
    Skipped(String),
    /// Refused or failed (bad signature, hash error, ...); the installation was rolled back / not changed.
    Failed(String),
}

pub struct Setup<'a> {
    pub public_key: &'a str,
    pub base_url: &'a str,
    pub curl: &'a Path,
    /// The game folder (directory of the client executable).
    pub root: &'a Path,
    /// The patch work folder (`<base>/patches`).
    pub work: &'a Path,
}

fn classify(e: PatchError) -> Outcome {
    match e {
        PatchError::Network(m) => Outcome::Offline(m),
        PatchError::Busy(m) => Outcome::Skipped(m),
        PatchError::Cancelled => Outcome::Skipped("cancelled".into()),
        other => Outcome::Failed(other.to_string()),
    }
}

pub fn valid_source(url: &str) -> bool {
    url.starts_with("https://") && url.ends_with('/')
}

fn fetch_manifest(s: &Setup) -> Result<Manifest, PatchError> {
    if s.public_key.trim().is_empty() {
        return Err(PatchError::Signature("no trusted public key".into()));
    }
    if !valid_source(s.base_url) {
        return Err(PatchError::Network("the patch source must be an https URL ending in /".into()));
    }
    let f = CurlFetch { curl: s.curl.to_path_buf(), base_url: s.base_url.to_string() };
    let text = f.get_text("manifest.txt", tsw_patch::manifest::MAX_MANIFEST_BYTES)?;
    let signature = f.get_text("manifest.txt.minisig", 4096)?;
    sig::verify(text.as_bytes(), &signature, s.public_key)?;
    Manifest::parse(&text)
}

/// Look only: what would an update do?
pub fn check(s: &Setup) -> Outcome {
    if s.public_key.trim().is_empty() {
        return Outcome::Disabled("the launcher contains no trusted patch key yet".into());
    }
    let installed = match apply::installed_version(s.work) {
        Ok(v) => v,
        Err(e) => return classify(e),
    };
    match fetch_manifest(s) {
        Ok(m) if m.version <= installed => Outcome::UpToDate { installed },
        Ok(m) => Outcome::Available { installed, latest: m.version },
        Err(e) => classify(e),
    }
}

/// Fetch, verify and apply. `game_running` is asked first; nothing is changed while the game runs.
pub fn update(s: &Setup, game_running: &dyn Fn() -> bool, cancel: &AtomicBool) -> Outcome {
    if s.public_key.trim().is_empty() {
        return Outcome::Disabled("the launcher contains no trusted patch key yet".into());
    }
    let installed = match apply::installed_version(s.work) {
        Ok(v) => v,
        Err(e) => return classify(e),
    };
    let m = match fetch_manifest(s) {
        Ok(m) => m,
        Err(e) => return classify(e),
    };
    if m.version <= installed {
        return Outcome::UpToDate { installed };
    }
    let fetcher = CurlFetch { curl: s.curl.to_path_buf(), base_url: s.base_url.to_string() };
    match apply::apply(s.root, s.work, &m, &fetcher, cancel, &Options { game_running: Some(game_running), ..Default::default() }) {
        Ok(files) => Outcome::Applied { from: installed, to: m.version, files },
        Err(e) => classify(e),
    }
}

/// Is a TSW process running? Looks at the command lines below `proc_dir` (`/proc`).
pub fn game_running_in(proc_dir: &Path) -> bool {
    let Ok(rd) = std::fs::read_dir(proc_dir) else {
        return false;
    };
    rd.flatten()
        .filter(|e| e.file_name().to_string_lossy().chars().all(|c| c.is_ascii_digit()))
        .any(|e| std::fs::read(e.path().join("cmdline")).map(|c| String::from_utf8_lossy(&c).to_ascii_lowercase()).is_ok_and(|c| c.contains("thesecretworld") || c.contains("clientpatcher")))
}

pub fn describe(o: &Outcome) -> String {
    match o {
        Outcome::Disabled(m) => format!("client patches: off ({m})"),
        Outcome::UpToDate { installed } => {
            format!("client patches: up to date (patch {installed})")
        }
        Outcome::Available { installed, latest } => {
            format!("client patches: patch {latest} is available (installed: {installed})")
        }
        Outcome::Applied { from, to, files } => format!("client patches: updated from {from} to {to} ({files} file(s) changed, all verified)"),
        Outcome::Offline(m) => {
            format!("client patches: source not reachable, playing with the installed state ({m})")
        }
        Outcome::Skipped(m) => format!("client patches: skipped ({m})"),
        Outcome::Failed(m) => format!("client patches: NOT applied, installation unchanged ({m})"),
    }
}

/// Where the patch work folder lives: `<install base>/patches` for an installation made by the installation manager, else below the data folder.
pub fn work_dir(state_base: Option<&Path>, data_dir: Option<&Path>) -> Option<PathBuf> {
    state_base.map(|b| b.join("patches")).or_else(|| data_dir.map(|d| d.join("tsw-launch").join("patches")))
}

/// `tsw-launch patch <status|check|apply|verify>`
pub fn run(args: Vec<String>) -> Result<u8, String> {
    use crate::discover::{self, Env};
    let cmd = args.first().map(String::as_str).ok_or("patch needs a command: status, check, apply, verify")?;
    let env = Env::from_process();
    let cfg_path = crate::config::default_path();
    let cfg = match &cfg_path {
        Some(p) => crate::config::load(p).map_err(|e| e.to_string())?,
        None => crate::config::Config::default(),
    };
    let explicit = crate::config::Config { client_exe: cfg.client_exe.clone(), runner: cfg.runner.clone(), prefix: cfg.prefix.clone(), ..Default::default() };
    let d = discover::resolve(&env, &explicit, discover::cache_path().as_deref()).map_err(|p| p.message())?;
    let root = d.client_exe.parent().ok_or("the client has no folder")?.to_path_buf();
    let state_base = env.install_base.clone();
    let data = env.xdg_data.clone().or_else(|| env.home.as_ref().map(|h| h.join(".local/share")));
    let work = work_dir(state_base.as_deref(), data.as_deref()).ok_or("cannot determine the patch work folder")?;
    let curl = crate::doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).unwrap_or_else(|| PathBuf::from("curl"));
    let base = cfg.patch_source.clone().unwrap_or_else(|| crate::project::PATCH_BASE_URL.to_string());
    let setup = Setup { public_key: crate::project::PATCH_PUBLIC_KEY, base_url: &base, curl: &curl, root: &root, work: &work };
    match cmd {
        "status" | "check" => {
            if cmd == "status" {
                // three different numbers: the launcher/engine (software), the manifest format, and the patch level of the client files
                let level = apply::installed_version(&work).map(|v| if v == 0 { "none (original client files)".to_string() } else { v.to_string() }).unwrap_or_else(|e| format!("unknown ({e})"));
                println!("{}\ninstalled client patch level: {level}", crate::project::version_line("tsw-launch"));
            }
            let o = check(&setup);
            println!("{}", describe(&o));
            Ok(if matches!(o, Outcome::Failed(_)) { 1 } else { 0 })
        }
        "apply" => {
            let o = update(&setup, &|| game_running_in(Path::new("/proc")), &AtomicBool::new(false));
            println!("{}", describe(&o));
            Ok(if matches!(o, Outcome::Failed(_)) { 1 } else { 0 })
        }
        "verify" => {
            let bad = apply::drift(&root, &work).map_err(|e| e.to_string())?;
            if bad.is_empty() {
                println!("client patches: installed files match patch {}", apply::installed_version(&work).map_err(|e| e.to_string())?);
                Ok(0)
            } else {
                println!("client patches: {} file(s) differ from the installed patch: {}", bad.len(), bad.join(", "));
                Ok(2)
            }
        }
        other => Err(format!("unknown patch command {other:?}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("tsw-patchcmd-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn without_a_trusted_key_everything_is_off_and_nothing_is_contacted() {
        let _exec = crate::test_exec_lock();
        let d = tmp("off");
        let s = Setup { public_key: "", base_url: crate::project::PATCH_BASE_URL, curl: Path::new("/nonexistent/curl"), root: &d, work: &d.join("patches") };
        assert!(matches!(check(&s), Outcome::Disabled(_)));
        assert!(matches!(update(&s, &|| false, &AtomicBool::new(false)), Outcome::Disabled(_)));
        assert!(!d.join("patches").exists(), "not even a work folder is created");
        assert_eq!(crate::project::PATCH_PUBLIC_KEY, "", "no key may be invented: the owner generates it");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[cfg(unix)]
    #[test]
    fn an_unreachable_source_or_a_bad_signature_never_blocks_play_and_changes_nothing() {
        let _exec = crate::test_exec_lock();
        use std::os::unix::fs::PermissionsExt;
        let d = tmp("net");
        let curl = d.join("curl");
        std::fs::write(&curl, "#!/bin/sh\necho 'Could not resolve host: github.com' >&2\nexit 6\n").unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        let key = "RWQf6LRCGA9i53mlYecO4IzT51TGPpvWucNSCh1CBM0QTaLn73Y7GFO3";
        let s = Setup { public_key: key, base_url: crate::project::PATCH_BASE_URL, curl: &curl, root: &d, work: &d.join("patches") };
        let o = check(&s);
        assert!(matches!(&o, Outcome::Offline(m) if m.contains("resolve host")), "{o:?}");
        assert!(describe(&o).contains("playing with the installed state"));
        assert!(matches!(update(&s, &|| false, &AtomicBool::new(false)), Outcome::Offline(_)));
        // a reachable source that serves garbage is a refusal, not an update
        let curl2 = d.join("curl2");
        std::fs::write(&curl2, "#!/bin/sh\necho garbage\n").unwrap();
        std::fs::set_permissions(&curl2, std::fs::Permissions::from_mode(0o755)).unwrap();
        let s2 = Setup { curl: &curl2, ..s };
        assert!(matches!(check(&s2), Outcome::Failed(m) if m.contains("signature")));
        let s3 = Setup { base_url: "http://example.org/", ..s2 };
        assert!(matches!(check(&s3), Outcome::Offline(m) if m.contains("https")), "plain http is never used for patches");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn a_running_game_is_recognised_from_process_command_lines() {
        let _exec = crate::test_exec_lock();
        let d = tmp("proc");
        std::fs::create_dir_all(d.join("123")).unwrap();
        std::fs::write(d.join("123/cmdline"), b"/usr/bin/bash\0-c\0x").unwrap();
        std::fs::create_dir_all(d.join("self")).unwrap();
        assert!(!game_running_in(&d));
        std::fs::create_dir_all(d.join("456")).unwrap();
        std::fs::write(d.join("456/cmdline"), b"C:\\Games\\TheSecretWorldDX11.exe\0-dx11").unwrap();
        assert!(game_running_in(&d));
        assert!(!game_running_in(&d.join("missing")));
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn the_work_folder_follows_the_installation_base() {
        let _exec = crate::test_exec_lock();
        assert_eq!(work_dir(Some(Path::new("/g/TSW")), Some(Path::new("/d"))), Some(PathBuf::from("/g/TSW/patches")));
        assert_eq!(work_dir(None, Some(Path::new("/d"))), Some(PathBuf::from("/d/tsw-launch/patches")));
        assert_eq!(work_dir(None, None), None);
        assert!(valid_source("https://x.org/p/") && !valid_source("http://x.org/p/") && !valid_source("https://x.org/p"));
    }
}
