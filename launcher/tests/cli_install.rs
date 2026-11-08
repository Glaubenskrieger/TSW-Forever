//! The `install` command through the real binary, in a private HOME: state, refusal of unsafe input, protection of a working
//! configuration. Nothing is downloaded and no installer or Wine program is run.

use std::path::{Path, PathBuf};
use std::process::Command;

struct Home {
    root: PathBuf,
}

impl Home {
    fn new(tag: &str) -> Home {
        let root = std::env::temp_dir().join(format!("tsw-launch-cli-install-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("home")).unwrap();
        Home { root }
    }
    fn run(&self, args: &[&str]) -> (i32, String, String) {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_tsw-launch"));
        cmd.arg("install").args(args).env("HOME", self.root.join("home")).env("XDG_CONFIG_HOME", self.root.join("cfg")).env_remove("TSW_LAUNCH_CONFIG").env("PATH", self.root.join("emptybin"));
        // native Windows keeps its settings in %APPDATA%	sw-forever (Known Folder), not in XDG_CONFIG_HOME: name the private file explicitly
        #[cfg(windows)]
        cmd.env("TSW_LAUNCH_CONFIG", self.config());
        let out = cmd.output().unwrap();
        (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
    }
    fn base(&self) -> PathBuf {
        self.root.join("home").join("Games").join("TheSecretWorld")
    }
    fn config(&self) -> PathBuf {
        self.root.join("cfg").join("tsw-launch").join("config")
    }
}

#[cfg(unix)] // Linux install layout (HOME/Games, Wine prefix); the Windows layout is covered by install::native_tests
#[test]
fn status_prepare_and_import_walk_through_the_early_steps_and_unsafe_bases_are_refused() {
    let h = Home::new("walk");
    let base = h.base();
    let b = base.to_str().unwrap();
    // no folder chosen: the default ~/Games/The Secret World below the (temporary) home, nothing is created by looking at it
    let (code, out, err) = h.run(&["status"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("Games/The Secret World") && out.contains("nothing created yet"), "{out}");
    assert!(!h.root.join("home").join("Games").exists(), "status creates nothing");
    for bad in ["/", "/usr/share/tsw", "relative/dir", h.root.join("home").to_str().unwrap()] {
        let (code, _, err) = h.run(&["prepare", "--base", bad]);
        assert_ne!(code, 0, "{bad}");
        assert!(!err.is_empty());
    }
    assert!(!base.exists());
    let (code, out, err) = h.run(&["prepare", "--base", b]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("folders ready"));
    for d in ["prefix", "downloads", "runtime", "cache", "logs"] {
        assert!(base.join(d).is_dir(), "{d}");
    }
    let (code, out, _) = h.run(&["status"]);
    assert_eq!(code, 0);
    assert!(out.contains("folders ready, no installer yet") && out.contains("a checksum is pinned") && out.contains("https://cdn.funcom.com/"), "{out}");

    // download is blocked without consent; with consent it would start curl, which is not on the PATH here, so nothing is fetched
    let (code, _, err) = h.run(&["download"]);
    assert_ne!(code, 0);
    assert!(err.contains("consent"), "{err}");
    let (code, _, err) = h.run(&["download", "--consent-download"]);
    assert_ne!(code, 0);
    assert!(err.contains("curl"), "{err}");
    assert!(!base.join("downloads/TheSecretWorldInstaller.exe.part").exists());

    // importing a file the user checked: unverified until compared
    let mine = h.root.join("mine.exe");
    std::fs::write(&mine, b"abc").unwrap();
    let (code, out, err) = h.run(&["import", "--file", mine.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad") && out.contains("NOT recognised"), "{out}");
    assert!(base.join("prefix").is_dir() && !base.join("game").exists(), "no game folder is created");
    assert!(h.run(&["import", "--file", mine.to_str().unwrap()]).0 != 0, "never replaces the cached installer");
    let (_, out, _) = h.run(&["status"]);
    assert!(out.contains("installer present, checksum NOT verified"), "{out}");
    // running it without a prefix or confirmation is refused
    assert_ne!(h.run(&["run"]).0, 0);
    let _ = std::fs::remove_dir_all(&h.root);
}

#[test]
fn adopt_and_finish_store_the_installation_but_never_replace_a_working_configuration() {
    let h = Home::new("adopt");
    let game = h.root.join("existing game").join("The Secret World");
    std::fs::create_dir_all(&game).unwrap();
    let exe = game.join("TheSecretWorldDX11.exe");
    std::fs::write(&exe, b"MZ").unwrap();
    let before = std::fs::read(&exe).unwrap();
    let (code, out, err) = h.run(&["adopt", "--path", game.parent().unwrap().to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("nothing was copied or changed"));
    let cfg = std::fs::read_to_string(h.config()).unwrap();
    assert!(cfg.contains(&format!("client_exe = {}", exe.display())), "{cfg}");
    assert_eq!(std::fs::read(&exe).unwrap(), before);
    // a second, different installation does not replace the working entry
    let other = h.root.join("other").join("TheSecretWorldDX11.exe");
    std::fs::create_dir_all(other.parent().unwrap()).unwrap();
    std::fs::write(&other, b"MZ").unwrap();
    let (code, _, err) = h.run(&["adopt", "--path", other.to_str().unwrap()]);
    assert_ne!(code, 0);
    assert!(err.contains("already points to a working installation"), "{err}");
    assert!(std::fs::read_to_string(h.config()).unwrap().contains(&exe.display().to_string()));
    // a folder without a client, and a wrong file name, are refused
    assert_ne!(h.run(&["adopt", "--path", h.root.to_str().unwrap().replace("tsw-launch-cli", "nothing-here").as_str()]).0, 0);
    let wrong = Path::new(&h.root).join("wrong.exe");
    std::fs::write(&wrong, b"x").unwrap();
    assert_ne!(h.run(&["adopt", "--path", wrong.to_str().unwrap()]).0, 0);
    let _ = std::fs::remove_dir_all(&h.root);
}

/// The game as the original installer and patcher leave it below the managed base: client and the RDB index (which exist from the first seconds of a download).
fn game_files(h: &Home) -> PathBuf {
    let game = h.base().join("prefix/drive_c/Program Files (x86)/Funcom/The Secret World");
    std::fs::create_dir_all(game.join("RDB")).unwrap();
    std::fs::write(game.join("TheSecretWorldDX11.exe"), b"MZ").unwrap();
    std::fs::write(game.join("RDB/RDBHashIndex.bin"), b"x").unwrap();
    game
}

#[cfg(unix)]
fn patcher_log(downloading: u32, roothash_new: &str) -> String {
    format!(
        "[t] [ID:0] [Main] Patcher exe hash is ab\n[t] [ID:0] [LocalRDBSource] New resources: 0 0.00 MB\n[t] [ID:0] [LocalRDBSource] Changed resources: 0 0.00 MB\n[t] [ID:0] [LocalRDBSource] Changed source hash resources: 0 0.00 MB\n[t] [ID:0] [LocalRDBSource] Deleted resources: 0 0.00 MB\n[t] [ID:0] [LocalRDBSource] Old roothash: aa\n[t] [ID:0] [LocalRDBSource] New roothash: {roothash_new}\n[t] [ID:0] [RDBManager] Downloading new: {downloading} 0.00 MB\n[t] [ID:0] [RDBManager] Downloading changed: 0 0.00 MB\n[t] [ID:0] [RDBManager] Downloading patches: 0 0.00 MB\n"
    )
}

#[cfg(unix)] // Linux install layout (HOME/Games, Wine prefix); the Windows layout is covered by install::native_tests
#[test]
fn the_command_line_judges_the_installation_like_the_window_and_never_forces_a_completion() {
    let h = Home::new("gate");
    let b = h.base();
    assert_eq!(h.run(&["prepare", "--base", b.to_str().unwrap()]).0, 0);
    let game = game_files(&h);
    let exe = game.join("TheSecretWorldDX11.exe");
    let stored = |h: &Home| std::fs::read_to_string(h.config()).map(|c| c.contains("client_exe")).unwrap_or(false);

    // client and index exist (as they do 30 s into the first patcher run) but nothing confirmed it: NOT installed, and the command line says why it cannot tell
    let (code, out, _) = h.run(&["status"]);
    assert_eq!(code, 0);
    assert!(out.contains("completion is NOT confirmed") && out.contains("cannot tell a finished download from an interrupted one"), "{out}");
    assert!(out.contains("next:") && out.contains("install patcher") && !out.contains("installed:"), "{out}");
    let (code, _, err) = h.run(&["finish"]);
    assert_ne!(code, 0);
    assert!(err.contains("not confirmed as complete") && err.contains("Nothing was stored"), "{err}");
    assert!(!stored(&h), "nothing is stored without the evidence");

    // an interrupted or unfinished download: the patcher's last run reported data to fetch
    std::fs::write(game.join("Patcher.log"), patcher_log(5912, "aa")).unwrap();
    let (_, out, _) = h.run(&["status"]);
    assert!(out.contains("the last run reported data to fetch") && !out.contains("installed:"), "{out}");
    assert_ne!(h.run(&["finish"]).0, 0);
    assert!(!stored(&h));

    // a different roothash is not complete either, even with nothing to fetch
    std::fs::write(game.join("Patcher.log"), patcher_log(0, "bb")).unwrap();
    assert_ne!(h.run(&["finish"]).0, 0);
    assert!(!stored(&h));

    // the patcher's own summary: nothing left to fetch and the same roothash: confirmed, exactly the window's condition
    std::fs::write(game.join("Patcher.log"), patcher_log(0, "aa")).unwrap();
    let (_, out, _) = h.run(&["status"]);
    assert!(out.contains("installed:") && out.contains("install finish"), "{out}");
    let (code, out, err) = h.run(&["finish"]);
    assert_eq!(code, 0, "{err}");
    assert!(out.contains("installation stored"), "{out}");
    assert!(std::fs::read_to_string(h.config()).unwrap().contains(&format!("client_exe = {}", exe.display())));
    let _ = std::fs::remove_dir_all(&h.root);
}

#[test]
fn a_client_the_user_names_is_the_users_own_decision_and_needs_no_confirmation() {
    let h = Home::new("gate-explicit");
    let b = h.base();
    assert_eq!(h.run(&["prepare", "--base", b.to_str().unwrap()]).0, 0);
    let game = game_files(&h);
    let exe = game.join("TheSecretWorldDX11.exe");
    let (code, _, err) = h.run(&["finish", "--client", exe.to_str().unwrap()]);
    assert_eq!(code, 0, "{err}");
    assert!(std::fs::read_to_string(h.config()).unwrap().contains("client_exe"));
    let _ = std::fs::remove_dir_all(&h.root);
}
