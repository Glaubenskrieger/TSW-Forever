//! End-to-end check of the command line the CLI builds (`--dry-run` starts nothing): the normal path without Gamescope, and the optional
//! plain Gamescope window with a stand-in `gamescope` on a private PATH. Everything lives in a temporary directory; no game, Wine or gamescope is run
//! except the stand-in shell scripts (`gamescope --help` answers with a fixture text).

use std::path::{Path, PathBuf};
use std::process::Command;

fn tmp(tag: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("tsw-launch-cli-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

/// A fixture "client": the two marker strings and four key words that make the key search succeed (same shape as the unit-test sample).
fn fixture_exe(path: &Path) {
    let mut buf = b"MZ....Invalid patcher key arg \0\0\0".to_vec();
    for w in [0x5bce568eu32, 0xfb2e8cbc, 0xa324e6d8, 0xf22bf865] {
        buf.extend_from_slice(&w.to_le_bytes());
    }
    buf.extend_from_slice(b"Could not decode patcher key \0");
    std::fs::write(path, buf).unwrap();
}

fn script(path: &Path, body: &str) {
    use std::os::unix::fs::PermissionsExt;
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

struct Fixture {
    root: PathBuf,
    exe: PathBuf,
    wine: PathBuf,
    prefix: PathBuf,
    bin: PathBuf,
    config: PathBuf,
}

fn fixture(tag: &str) -> Fixture {
    let root = tmp(tag);
    let (game, bin, prefix) = (root.join("game dir"), root.join("bin"), root.join("prefix"));
    for d in [&game, &bin, &prefix] {
        std::fs::create_dir_all(d).unwrap();
    }
    let exe = game.join("TheSecretWorldDX11.exe");
    fixture_exe(&exe);
    let wine = bin.join("wine");
    script(&wine, "exit 0");
    Fixture { config: root.join("config"), root, exe, wine, prefix, bin }
}

fn dry_run(f: &Fixture, config: &str, extra: &[&str]) -> (i32, String, String) {
    std::fs::write(&f.config, config).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_tsw-launch"))
        .args(["--config"])
        .arg(&f.config)
        .args(["--exe"])
        .arg(&f.exe)
        .args(["--runner"])
        .arg(&f.wine)
        .args(["--prefix"])
        .arg(&f.prefix)
        .args(["--now", "1793300058", "--dry-run"])
        .args(extra)
        .env("PATH", &f.bin)
        .env("HOME", &f.root)
        .env("XDG_CONFIG_HOME", f.root.join("xdg"))
        .env("XDG_DATA_HOME", f.root.join("xdgdata"))
        .env_remove("WINEPREFIX")
        .env_remove("GAMESCOPE_WAYLAND_DISPLAY")
        .env_remove("FLATPAK_ID")
        .env_remove("XDG_CURRENT_DESKTOP")
        .output()
        .unwrap();
    (out.status.code().unwrap_or(-1), String::from_utf8_lossy(&out.stdout).into_owned(), String::from_utf8_lossy(&out.stderr).into_owned())
}

fn program_line(stdout: &str) -> String {
    stdout.lines().find_map(|l| l.strip_prefix("program: ")).unwrap_or("").to_string()
}

#[test]
fn without_gamescope_the_start_path_is_plain_wine_and_nothing_mentions_gamescope() {
    let f = fixture("off");
    let (code, out, err) = dry_run(&f, "dx = 11\n", &[]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(program_line(&out), f.wine.display().to_string(), "golden: the program is Wine itself");
    assert!(!out.contains("gamescope") && !err.contains("gamescope"), "{out}\n{err}");
    // golden: the complete dry-run report of the normal path (client arguments, working directory, environment names)
    let names: Vec<&str> = out.lines().filter_map(|l| l.strip_prefix("env: ")).map(|l| l.split('=').next().unwrap()).collect();
    assert_eq!(names, ["WINE_NO_WM_DECORATION", "DXVK_CONFIG"], "golden: the Alt+Tab fix (no window-manager decorations) and the frame-rate limit: {out}");
    assert!(out.lines().any(|l| l == format!("dir: {}", f.exe.parent().unwrap().display())), "{out}");
    assert!(!out.contains("DisplayResolution"), "default launch must preserve the game's saved resolution: {out}");
    assert!(!out.contains("DisplayMode"), "the display mode is left alone unless asked for: {out}");
    assert!(out.contains("-dx11"));
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn explicit_no_gamescope_overrides_a_config_that_turned_it_on() {
    let f = fixture("override");
    let (code, out, err) = dry_run(&f, "gamescope = true\ngamescope_output = 1920x1200\n", &["--no-gamescope"]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(program_line(&out), f.wine.display().to_string());
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn old_fsr_settings_in_a_config_are_read_and_have_no_effect() {
    let f = fixture("legacy");
    script(&f.bin.join("gamescope"), "exit 0");
    let (code, out, err) = dry_run(&f, "fsr = true\nfsr_internal = 1280x800\nfsr_output = 1920x1200\nfsr_sharpness = 7\n", &[]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(program_line(&out), f.wine.display().to_string(), "an old fsr = true neither switches gamescope on nor changes the start");
    assert!(!out.contains("gamescope"), "{out}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn gamescope_on_but_not_installed_refuses_with_a_message_that_names_gamescope_and_the_way_out() {
    let f = fixture("missing");
    let (code, out, err) = dry_run(&f, "gamescope = true\ngamescope_output = 1920x1200\n", &[]);
    assert_eq!(code, 3, "{out}\n{err}");
    assert!(err.contains("Gamescope is switched on, but was not found") && err.contains("the normal start needs neither"), "{err}");
    assert!(!err.contains("FSR"), "{err}");
    let (code, _, err) = dry_run(&f, "dx = 11\n", &[]);
    assert_eq!(code, 0, "the normal start does not need gamescope: {err}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn gamescope_wraps_wine_with_identical_internal_and_output_size_and_no_filter() {
    let f = fixture("on");
    script(&f.bin.join("gamescope"), "exit 0");
    let (code, out, err) = dry_run(&f, "gamescope = true\ngamescope_output = 1920x1200\n", &[]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(program_line(&out), f.bin.join("gamescope").display().to_string());
    assert!(out.contains("gamescope: 1920x1200 (internal = output, no scaling)"), "{out}");
    assert!(!out.contains("fsr") && !out.contains("sharpness"), "{out}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn gamescope_with_a_virtual_desktop_or_a_malformed_size_is_refused_before_anything_starts() {
    let f = fixture("bad");
    script(&f.bin.join("gamescope"), "exit 0");
    let (code, _, err) = dry_run(&f, "gamescope = true\ngamescope_output = 1920x1200\nvirtual_desktop = 1920x1200\n", &[]);
    assert_ne!(code, 0);
    assert!(err.contains("virtual desktop"), "{err}");
    let (code, _, err) = dry_run(&f, "", &["--gamescope", "--gamescope-output", "1280x720; touch /tmp/x"]);
    assert_ne!(code, 0);
    assert!(err.contains("must look like"), "{err}");
    let (code, _, err) = dry_run(&f, "", &["--fsr"]);
    assert_ne!(code, 0, "the FSR option is gone: {err}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn the_display_mode_is_only_touched_when_asked_for_and_works_with_and_without_gamescope() {
    let f = fixture("dm");
    script(&f.bin.join("gamescope"), "exit 0");
    for (cfg, extra) in [("display_mode = borderless\n", vec![]), ("gamescope = true\ngamescope_output = 1920x1200\n", vec!["--display-mode", "borderless"])] {
        let (code, out, err) = dry_run(&f, cfg, &extra);
        assert_eq!(code, 0, "{out}\n{err}");
        assert!(out.contains("prefs: DisplayMode borderless would be"), "{out}");
    }
    let (_, out, _) = dry_run(&f, "dx = 11\n", &[]);
    assert!(!out.contains("DisplayMode"));
    let (code, _, err) = dry_run(&f, "", &["--display-mode", "exclusive"]);
    assert_ne!(code, 0);
    assert!(err.contains("display_mode must be"), "{err}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn stay_alive_keeps_the_launcher_running_until_the_client_has_ended_and_plain_detach_returns_at_once() {
    let f = fixture("stay");
    script(&f.wine, "/bin/sleep 2");
    let run = |extra: &[&str]| {
        std::fs::write(&f.config, "dx = 11\n").unwrap();
        let t = std::time::Instant::now();
        let out = Command::new(env!("CARGO_BIN_EXE_tsw-launch"))
            .arg("--config")
            .arg(&f.config)
            .arg("--exe")
            .arg(&f.exe)
            .arg("--runner")
            .arg(&f.wine)
            .arg("--prefix")
            .arg(&f.prefix)
            .args(["--now", "1793300058", "--detach"])
            .args(extra)
            .env("PATH", &f.bin)
            .env("HOME", &f.root)
            .env("XDG_CONFIG_HOME", f.root.join("xdg"))
            .env("XDG_DATA_HOME", f.root.join("xdgdata"))
            .env_remove("WINEPREFIX")
            .env_remove("FLATPAK_ID")
            .output()
            .unwrap();
        (out.status.code(), String::from_utf8_lossy(&out.stderr).into_owned(), t.elapsed())
    };
    let (code, err, took) = run(&[]);
    assert_eq!(code, Some(0), "{err}");
    assert!(took < std::time::Duration::from_millis(1500), "plain --detach returns at once: {took:?}");
    let (code, err, took) = run(&["--stay-alive"]);
    assert_eq!(code, Some(0), "{err}");
    assert!(err.contains("tsw-launch: started (pid"), "the front end waits for this line: {err}");
    assert!(took >= std::time::Duration::from_millis(1800), "--stay-alive waits for the client: {took:?}");
    let _ = std::fs::remove_dir_all(&f.root);
}

fn env_names(out: &str) -> Vec<String> {
    out.lines().filter_map(|l| l.strip_prefix("env: ")).map(|l| l.split('=').next().unwrap().to_string()).collect()
}

fn has_wm_switch(out: &str) -> bool {
    env_names(out).iter().any(|n| n == "WINE_NO_WM_DECORATION")
}

#[test]
fn the_alt_tab_fix_removes_window_manager_decorations_of_a_full_screen_wine_game_and_can_be_switched_off() {
    let f = fixture("wmdeco");
    let (code, out, err) = dry_run(&f, "dx = 11\n", &[]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert!(has_wm_switch(&out), "default (auto): {out}");
    let (_, out, _) = dry_run(&f, "dx = 11\n", &["--wm-decorations", "on"]);
    assert!(!has_wm_switch(&out), "on keeps the decorations: {out}");
    let (_, out, _) = dry_run(&f, "dx = 11\nwm_decorations = on\n", &[]);
    assert!(!has_wm_switch(&out), "configuration key on: {out}");
    let (_, out, _) = dry_run(&f, "dx = 11\nwm_decorations = on\n", &["--wm-decorations", "off"]);
    assert!(has_wm_switch(&out), "the command line beats the file: {out}");
    let (code, _, err) = dry_run(&f, "dx = 11\n", &["--wm-decorations", "sometimes"]);
    assert_ne!(code, 0);
    assert!(err.contains("wm_decorations must be"), "{err}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn auto_keeps_the_decorations_for_a_windowed_game_a_virtual_desktop_and_a_user_defined_value() {
    let f = fixture("wmauto");
    let (_, out, _) = dry_run(&f, "display_mode = windowed\n", &[]);
    assert!(!has_wm_switch(&out), "explicit windowed mode: {out}");
    let (_, out, _) = dry_run(&f, "display_mode = borderless\n", &[]);
    assert!(has_wm_switch(&out), "borderless is not windowed: {out}");
    let (_, out, _) = dry_run(&f, "virtual_desktop = 1920x1200\n", &[]);
    assert!(!has_wm_switch(&out), "virtual desktop: {out}");
    let (_, out, _) = dry_run(&f, "env.WINE_NO_WM_DECORATION = 0\n", &[]);
    assert_eq!(env_names(&out).iter().filter(|n| *n == "WINE_NO_WM_DECORATION").count(), 1, "the user's own value is used once, not overridden: {out}");
    // the client's own saved preference decides when no display mode is configured
    let prefs = f.prefix.join("drive_c/users/steamuser/AppData/Local/Funcom/TSW/Prefs");
    std::fs::create_dir_all(&prefs).unwrap();
    std::fs::write(prefs.join("Prefs_3.xml"), "<Root>\n  <Value name=\"DisplayMode\" value=\"1\" />\n</Root>\n").unwrap();
    let (_, out, _) = dry_run(&f, "dx = 11\n", &[]);
    assert!(!has_wm_switch(&out), "saved DisplayMode 1 = windowed: {out}");
    std::fs::write(prefs.join("Prefs_3.xml"), "<Root>\n  <Value name=\"DisplayMode\" value=\"0\" />\n</Root>\n").unwrap();
    let (_, out, _) = dry_run(&f, "dx = 11\n", &[]);
    assert!(has_wm_switch(&out), "saved DisplayMode 0 = full screen: {out}");
    let _ = std::fs::remove_dir_all(&f.root);
}

#[test]
fn explicit_game_managed_selection_ignores_but_does_not_delete_legacy_display_settings() {
    let f = fixture("game-managed");
    let text = "game_resolution = game\nresolution = 3840x2160\nvirtual_desktop = 3840x2160\ngamescope = true\n";
    let (code, out, err) = dry_run(&f, text, &[]);
    assert_eq!(code, 0, "{out}\n{err}");
    assert_eq!(program_line(&out), f.wine.display().to_string());
    assert!(!out.contains("/desktop=") && !out.contains("DisplayResolution"), "{out}");
    assert_eq!(std::fs::read_to_string(&f.config).unwrap(), text, "no stored legacy keys are erased");
}
