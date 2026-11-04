//! `tsw-launch install ...`: command-line front end of the installation manager ([`crate::install`]).
//!
//! ```text
//! install status   [--base DIR]            where the installation stands and what comes next (read-only)
//! install prepare  --base DIR              create <base>/game, prefix, downloads (refuses non-empty game/prefix)
//! install download [--base DIR] --consent-download [--sha256 HEX]
//!                                          download the installer (needs consent AND a pinned checksum)
//! install import   --file F [--sha256 HEX] copy an installer you downloaded and checked yourself into the cache
//! install prefix   [--runner WINE]         create the Wine/Proton prefix with the launcher's prefix manager
//! install patcher                         start the original ClientPatcher.exe if the installer left only the patcher (it fetches the game data)
//! install run      [--runner WINE] [--confirm-sha256 HEX]   start the original installer in the prefix (keep its default settings and path)
//! install adopt    --path EXE_OR_DIR [--prefix DIR]     use an existing installation (nothing is copied or changed)
//! install runner  [--consent-download]     provide the pinned GE-Proton runner in the launcher's own data folder (status without the flag)
//! install shaders [--consent-download] [--accept-microsoft-license]     install the native Direct3D profile into the prefix (components from Microsoft, verified)
//! install import-rdb --from DIR [--overwrite-existing] [--allow-copy]   take over an RDB folder YOU name (reflink clone, source untouched, nothing searched)
//! install finish                           store the finished installation in the launcher configuration
//! ```
//! Every step is explicit; none runs implicitly. Output is plain text (German front ends translate [`crate::install`] results themselves).

use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::config;
use crate::discover::{self, Env};
use crate::install::{self, Downloader, Layout, Next, Stage, State};
use crate::progress::{Sink, Update};
use crate::runtimedl;

#[derive(Default, Debug)]
struct Args {
    base: Option<PathBuf>,
    file: Option<PathBuf>,
    path: Option<PathBuf>,
    /// `import-rdb`: replace files that already exist (the user's confirmation).
    overwrite: bool,
    /// `import-rdb`: allow full copies where a reflink is impossible.
    allow_copy: bool,
    prefix: Option<PathBuf>,
    runner: Option<PathBuf>,
    sha256: Option<String>,
    consent: bool,
    accept_microsoft_license: bool,
    /// SHA-256 of the one installer file the user confirms to run although it is not known to be genuine.
    confirm_sha256: Option<String>,
    /// Client executable chosen by hand when the automatic detection finds nothing.
    client: Option<PathBuf>,
}

fn parse(args: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = args.iter();
    while let Some(k) = it.next() {
        let mut val = |name: &str| it.next().cloned().ok_or_else(|| format!("{name} needs a value"));
        match k.as_str() {
            "--base" => a.base = Some(PathBuf::from(val("--base")?)),
            "--file" => a.file = Some(PathBuf::from(val("--file")?)),
            "--path" => a.path = Some(PathBuf::from(val("--path")?)),
            "--from" => a.file = Some(PathBuf::from(val("--from")?)),
            "--overwrite-existing" => a.overwrite = true,
            "--allow-copy" => a.allow_copy = true,
            "--prefix" => a.prefix = Some(PathBuf::from(val("--prefix")?)),
            "--runner" => a.runner = Some(PathBuf::from(val("--runner")?)),
            "--sha256" => {
                let v = val("--sha256")?;
                if !crate::sha256::is_hex_digest(&v) {
                    return Err("--sha256 needs 64 hexadecimal digits".into());
                }
                a.sha256 = Some(v.to_ascii_lowercase());
            }
            "--consent-download" => a.consent = true,
            "--accept-microsoft-license" => a.accept_microsoft_license = true,
            "--confirm-sha256" => {
                let v = val("--confirm-sha256")?;
                if !crate::sha256::is_hex_digest(&v) {
                    return Err("--confirm-sha256 needs 64 hexadecimal digits".into());
                }
                a.confirm_sha256 = Some(v.to_ascii_lowercase());
            }
            "--client" => a.client = Some(PathBuf::from(val("--client")?)),
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(a)
}

struct Printer;
impl Sink for Printer {
    fn update(&self, u: Update) {
        if let Update::Progress(p) = u {
            if let Some(pc) = p.percent() {
                eprint!("\r{} {} %   ", p.step, pc);
            }
        }
    }
}

fn pick_runner(explicit: Option<PathBuf>) -> Result<PathBuf, String> {
    if let Some(r) = explicit {
        return Ok(r);
    }
    discover::find_runtimes(&Env::from_process())
        .into_iter()
        .next()
        .map(|r| r.wine)
        .ok_or_else(|| "no Wine/Proton runtime found (searched Steam compatibility tools); pass --runner <path to wine>".to_string())
}

/// Does the launcher configuration already name a working client (an installation the user chose or an earlier, finished one)?
fn adopted_in_config() -> bool {
    config::write_path().and_then(|p| config::load(&p).ok()).and_then(|c| c.client_exe).is_some_and(|p| p.is_file())
}

fn describe(layout: &Layout, state: &State) -> String {
    describe_with(layout, state, &install::gather_evidence(layout, state), adopted_in_config())
}

/// The same judgement as the window (`install::gate_for`), without a watcher: the command line cannot see a patcher run, so a finished download cannot be told
/// from an interrupted one here and is reported as "not confirmed" unless the patcher's own summary (or the user's own choice) says so.
fn describe_with(layout: &Layout, state: &State, ev: &install::Evidence, adopted: bool) -> String {
    let sha = layout.installer().is_file().then(|| crate::sha256::file_sha256(&layout.installer()).ok().map(|(h, _)| h)).flatten();
    let a = install::assess(layout, state, sha.as_deref());
    let gate = install::gate_for(a.next, adopted, ev, false);
    let stage = match &a.stage {
        Stage::Empty => "nothing created yet".to_string(),
        Stage::FoldersReady => "folders ready, no installer yet".to_string(),
        Stage::PartialDownload { bytes } => format!("download interrupted at {} (will be resumed)", crate::progress::format_bytes(*bytes)),
        Stage::InstallerReady { checksum: install::Checksum::Verified } => "installer present, checksum verified".to_string(),
        Stage::InstallerReady { .. } => format!("installer present, checksum NOT verified (SHA-256 {})", sha.as_deref().unwrap_or("unknown")),
        Stage::PrefixReady => "prefix ready, installer not run yet".to_string(),
        Stage::PatcherReady { patcher } => format!("only the original patcher is installed ({}); run it to fetch the game data", patcher.display()),
        Stage::Installed { client } => match gate {
            install::Gate::Open => format!("installed: {}", client.display()),
            install::Gate::Working => format!("files of the game exist ({}), but a Funcom program is working: completion is not confirmed yet", client.display()),
            install::Gate::Unverified => format!(
                "files of the game exist ({}), but completion is NOT confirmed ({}); this command line cannot observe a patcher run, so it cannot tell a finished download from an interrupted one",
                client.display(),
                install::verdict_words(ev.verdict)
            ),
        },
    };
    let next = match a.next {
        Next::PrepareFolders => "install prepare",
        Next::GetInstaller => "install download (or install import)",
        Next::CreatePrefix => "install prefix",
        Next::RunInstaller => "install run",
        Next::RunPatcher => "install patcher",
        Next::Play => match gate {
            install::Gate::Open => "install finish, then start the game",
            install::Gate::Working => "wait: a Funcom program is working (it is not interrupted); look again afterwards",
            install::Gate::Unverified => {
                "install patcher (the original patcher checks and repairs; it confirms in its log when nothing is left to fetch), then look again; the window does all of this by itself"
            }
        },
    };
    let src = match state.expected_sha256(&install::OFFICIAL) {
        Some(_) => "a checksum is pinned (trust on first use, see the documentation): a download is possible after consent",
        None => "no trusted checksum known: automatic download is blocked; import your own checked installer",
    };
    let legacy = if install::is_legacy_base(&layout.base) {
        "\nnote:   this folder is an EARLIER layout (inside the Flatpak data folder): it is only reported, never used or changed by the launcher. Choose the default ~/Games/The Secret World; moving old data is a deliberate manual step."
    } else {
        ""
    };
    format!("base:   {}{legacy}\nstage:  {stage}\nnext:   {next}\nsource: {} ({src})", layout.base.display(), install::OFFICIAL.url)
}

/// Entry point for `tsw-launch install <args>`. Returns the process exit code.
pub fn run(raw: Vec<String>) -> Result<u8, String> {
    let (cmd, rest) = raw.split_first().ok_or("install needs a command: status, prepare, download, import, runner, prefix, run, adopt, finish")?;
    let a = parse(rest)?;
    let state_file = install::state_path().ok_or("cannot determine where to store the installation state (no HOME / XDG_CONFIG_HOME)")?;
    let mut state = State::load(&state_file)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(b) = &a.base {
        let l = Layout::new(b, home.as_deref()).map_err(|e| e.to_string())?;
        if state.base.as_ref() != Some(&l.base) {
            state.base = Some(l.base);
            state.save(&state_file)?;
        }
    }
    let layout = || -> Result<Layout, String> {
        // no folder chosen yet: the default ~/Games/The Secret World
        let base = state.base.clone().or_else(|| install::default_base(home.as_deref())).ok_or("no home folder: pass --base <folder>")?;
        if install::is_legacy_base(&base) && cmd.as_str() != "status" {
            return Err(format!("{} belongs to an EARLIER layout (inside the Flatpak data folder). It is not used or changed; run `install status` for details. Use the default ~/Games/The Secret World (pass --base to choose another folder).", base.display()));
        }
        Layout::new(&base, home.as_deref()).map_err(|e| e.to_string())
    };
    let e = |x: install::InstallError| x.to_string();
    match cmd.as_str() {
        "status" => {
            println!("{}", describe(&layout()?, &state));
            Ok(0)
        }
        "prepare" => {
            let l = layout()?;
            install::prepare(&l).map_err(e)?;
            println!("folders ready below {}", l.base.display());
            Ok(0)
        }
        "download" => {
            let l = layout()?;
            if a.consent {
                state.download_consent = Some(install::OFFICIAL.url.to_string());
            }
            if let Some(h) = &a.sha256 {
                state.pinned_sha256 = Some(h.clone());
            }
            state.save(&state_file)?;
            install::download_gate(&install::OFFICIAL, &state).map_err(e)?;
            let curl = crate::doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).ok_or("curl was not found on PATH")?;
            let cancel = AtomicBool::new(false);
            let dest = install::download(&l, &install::OFFICIAL, &state, &Downloader { curl: &curl, cancel: &cancel, sink: &Printer }).map_err(e)?;
            eprintln!();
            println!("installer verified and stored: {}", dest.display());
            Ok(0)
        }
        "import" => {
            let l = layout()?;
            let from = a.file.clone().ok_or("install import needs --file <installer>")?;
            if let Some(h) = &a.sha256 {
                state.pinned_sha256 = Some(h.clone());
                state.save(&state_file)?;
            }
            let (dest, hash) = install::import_installer(&l, &from, state.pinned_sha256.as_deref()).map_err(e)?;
            println!(
                "copied to {}\nSHA-256: {hash}\n{}",
                dest.display(),
                if state.recognises(&install::OFFICIAL, &hash) {
                    "checksum matches the known value"
                } else {
                    "NOT recognised: running it needs your confirmation of exactly this fingerprint (install confirm)"
                }
            );
            Ok(0)
        }
        "runner" => {
            let rt = layout()?.runtime();
            let r = &runtimedl::GE_PROTON;
            if runtimedl::is_installed(&rt, r) {
                println!("runner present: {}", rt.join(r.dir_name).display());
                return Ok(0);
            }
            if !a.consent {
                println!(
                    "no launcher-managed runner yet. It would download {} ({} MB, SHA-256 {}) from {} into {}.\nRun again with --consent-download to agree.",
                    r.dir_name,
                    r.size / 1_000_000,
                    r.sha256,
                    r.url,
                    rt.display()
                );
                return Ok(1);
            }
            let curl = crate::doctor::find_program(Path::new("curl"), std::env::var_os("PATH").as_deref()).ok_or("curl was not found on PATH")?;
            let tar = crate::doctor::find_program(Path::new("tar"), std::env::var_os("PATH").as_deref()).ok_or("tar was not found on PATH")?;
            let cancel = AtomicBool::new(false);
            let dir = runtimedl::install(&rt, r, true, &tar, &Downloader { curl: &curl, cancel: &cancel, sink: &Printer }).map_err(e)?;
            eprintln!();
            println!("runner installed: {}", dir.display());
            Ok(0)
        }
        "shaders" => {
            let l = layout()?;
            let prefix = l.prefix();
            let profile = &crate::prefix::PROFILE_D3D_NATIVE_MS2010;
            if let Some(id) = crate::prefix::installed_native(&prefix) {
                println!("a native shader profile is already installed: {id}");
                return Ok(0);
            }
            let native_cache = layout()?.native_cache();
            let path = std::env::var_os("PATH");
            let find = |n: &str| crate::doctor::find_program(Path::new(n), path.as_deref()).ok_or(format!("{n} was not found on PATH"));
            let (curl, bsdtar) = (find("curl")?, find("bsdtar")?);
            let cancel = AtomicBool::new(false);
            let dir = crate::dxredist::provide(
                &native_cache,
                profile,
                a.consent,
                &|terms| {
                    println!("{terms}");
                    a.accept_microsoft_license
                },
                &bsdtar,
                &Downloader { curl: &curl, cancel: &cancel, sink: &Printer },
            )
            .map_err(e)?;
            eprintln!();
            crate::prefix::install_native(profile, &dir, &prefix)?;
            println!("profile {} installed in {} (undo with `prefix native-remove`)", profile.id, prefix.display());
            Ok(0)
        }
        "prefix" => {
            let l = layout()?;
            install::create_prefix(&l, &pick_runner(a.runner.clone())?, &Printer).map_err(e)?;
            eprintln!();
            println!("prefix ready: {} (the game will be installed inside it, with the installer's default paths)", l.prefix().display());
            Ok(0)
        }
        "run" => {
            let l = layout()?;
            let sha = crate::sha256::file_sha256(&l.installer()).map_err(|x| format!("cannot read the installer: {x}"))?.0;
            if let Some(c) = &a.confirm_sha256 {
                if *c != sha {
                    return Err(format!("--confirm-sha256 does not match this installer (its SHA-256 is {sha})"));
                }
                state.confirmed_sha256 = Some(sha.clone());
                state.save(&state_file)?;
            }
            install::run_gate(&l, &state, &install::OFFICIAL, &sha).map_err(e)?;
            println!("{}", install::INSTALLER_NOTICE);
            let mut cmd = install::installer_command(&l, &pick_runner(a.runner.clone())?);
            let pid = crate::launch::spawn_detached(&mut cmd, None).map_err(|x| format!("cannot start the installer: {x}"))?;
            println!("the original installer was started (pid {pid}). Keep its default settings and path; afterwards run `install finish` (or pass --client if the game is not found).");
            Ok(0)
        }
        "patcher" => {
            let l = layout()?;
            let Some(patcher) = install::patcher_to_run(&install::assess(&l, &state, None).stage) else {
                return Err("there is no original patcher to start yet (see install status)".into());
            };
            let mut cmd = install::patcher_command(&l, &pick_runner(a.runner.clone())?, &patcher);
            let pid = crate::launch::spawn_detached(&mut cmd, None).map_err(|x| format!("cannot start the patcher: {x}"))?;
            println!("the original patcher was started (pid {pid}); it downloads the game data. Then run `install finish`.");
            Ok(0)
        }
        "import-rdb" => {
            let l = layout()?;
            let from = a.file.clone().or_else(|| a.path.clone()).ok_or("install import-rdb needs --from <folder with the RDB files> (nothing is searched)")?;
            let r = install::import_rdb(&l, &from, a.overwrite, a.allow_copy).map_err(e)?;
            println!(
                "RDB import: {} file(s) {} ({} MB), {} existing file(s) left untouched. The source was not changed. Completeness is NOT verified: run `install patcher` so the original ClientPatcher checks and completes the data.",
                r.copied.len(),
                if r.reflinked { "cloned (reflink)" } else { "copied" },
                r.bytes / 1_048_576,
                r.skipped_existing.len()
            );
            Ok(0)
        }
        "adopt" => {
            let path = a.path.clone().ok_or("install adopt needs --path <client exe or folder>")?;
            let exe = install::adopt(&path).map_err(e)?;
            save_adoption(&install::Adoption { client_exe: exe.clone(), prefix: a.prefix.clone() }).map_err(|x| x.to_string())?;
            println!("using the existing installation {} (nothing was copied or changed there)", exe.display());
            Ok(0)
        }
        "finish" => {
            let l = layout()?;
            if let Some(c) = &a.client {
                state.client_exe = Some(install::adopt(c).map_err(e)?);
                state.save(&state_file)?;
            }
            let adoption = install::adoption_for(&l, &state)
                .ok_or("no complete installation was found (client plus RDB/RDBHashIndex.bin); run the installer/patcher, or pass --client <path to TheSecretWorldDX11.exe>")?;
            // the same condition as the window: files alone are not a finished installation. A client the user names with --client is the user's own decision.
            let ev = install::gather_evidence(&l, &state);
            let sha = None;
            let next = install::assess(&l, &state, sha).next;
            match install::gate_for(next, a.client.is_some() || adopted_in_config(), &ev, false) {
                install::Gate::Open => {}
                install::Gate::Working => return Err("a Funcom program (patcher or installer) is still working; it is not interrupted. Nothing was stored. Look again with `install status` when it has ended.".into()),
                install::Gate::Unverified => {
                    return Err(format!(
                        "the installation is not confirmed as complete ({}), and this command line cannot observe a patcher run to tell a finished download from an interrupted one. Nothing was stored. Run `install patcher`, wait until its log reports that nothing is left to fetch, then `install status` and `install finish` again; the window does this by itself. If YOU know that the installation is complete, name it yourself with --client <path to TheSecretWorldDX11.exe>.",
                        install::verdict_words(ev.verdict)
                    ))
                }
            }
            save_adoption(&adoption)?;
            println!("installation stored in the launcher configuration: {}", adoption.client_exe.display());
            Ok(0)
        }
        other => Err(format!("unknown install command {other:?}")),
    }
}

/// Merge into the launcher configuration (protects a working entry) and save it atomically.
pub fn save_adoption(a: &install::Adoption) -> Result<(), String> {
    let path = config::write_path().ok_or("cannot determine the configuration file")?;
    let mut cfg = match config::load(&path) {
        Ok(c) => c,
        Err(config::ConfigError::Io(_)) if !path.exists() => config::Config::default(),
        Err(err) => return Err(err.to_string()),
    };
    install::merge_config(&mut cfg, a).map_err(|e| e.to_string())?;
    config::save(&path, &cfg).map_err(|e| e.to_string())
}

#[cfg(test)]
mod gate_tests {
    use super::*;
    use crate::install::Evidence;
    use crate::patchlog::Verdict;

    fn world(tag: &str) -> (Layout, State) {
        let base = std::env::temp_dir().join(format!("tsw-describe-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let layout = Layout { base };
        let game = install::game_path(&layout);
        std::fs::create_dir_all(game.join("RDB")).unwrap();
        std::fs::write(game.join("TheSecretWorldDX11.exe"), b"MZ").unwrap();
        std::fs::write(game.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
        (layout, State::default())
    }

    #[test]
    fn a_running_funcom_program_is_reported_as_working_and_never_as_installed() {
        let (layout, state) = world("working");
        let ev = Evidence { verdict: Verdict::Downloading, patcher_running: true, ..Default::default() };
        let text = describe_with(&layout, &state, &ev, false);
        assert!(text.contains("a Funcom program is working") && text.contains("not interrupted") && !text.contains("installed:"), "{text}");
        let _ = std::fs::remove_dir_all(&layout.base);
    }

    #[test]
    fn the_same_evidence_gives_the_command_line_and_the_window_the_same_gate() {
        let (layout, state) = world("same");
        for (verdict, running, adopted, open) in [
            (Verdict::Complete, false, false, true),
            (Verdict::Complete, true, false, true),
            (Verdict::Downloading, false, false, false),
            (Verdict::Unknown, false, false, false),
            (Verdict::Working, true, false, false),
            (Verdict::Downloading, false, true, true),
        ] {
            let ev = Evidence { verdict, patcher_running: running, ..Default::default() };
            let text = describe_with(&layout, &state, &ev, adopted);
            assert_eq!(text.contains("installed:"), open, "{verdict:?} running={running} adopted={adopted}: {text}");
            assert_eq!(install::gate_for(install::Next::Play, adopted, &ev, false) == install::Gate::Open, open);
        }
        let _ = std::fs::remove_dir_all(&layout.base);
    }
}
