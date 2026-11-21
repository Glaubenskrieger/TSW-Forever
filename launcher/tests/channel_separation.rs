//! Channel separation on the FINISHED program: a stable build contains nothing of the testing channel, a testing build says what it is.
//!
//! What a stable binary must never contain: the separate testing application ID, the marker string of the private testing overlay, and the name of
//! the overlay file. In the private repository the real values are read from `packaging/channels/testing.conf`; the public source has no such file and the
//! built-in defaults are used, so the check works everywhere.

use std::path::PathBuf;
use std::process::Command;

fn overlay_values() -> Vec<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("packaging").join("channels").join("testing.conf");
    let mut v: Vec<String> = vec!["org.tswlauncher.Launcher.Testing".into(), "tsw-forever-channel-testing-overlay".into(), "channels/testing.conf".into()];
    if let Ok(text) = std::fs::read_to_string(path) {
        for line in text.lines().filter(|l| !l.trim_start().starts_with('#')) {
            if let Some((k, val)) = line.split_once('=') {
                if matches!(k.trim(), "APP_ID" | "MARKER") && !val.trim().is_empty() {
                    v.push(val.trim().to_string());
                }
            }
        }
    }
    v
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

fn program() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tsw-launch"))
}

#[test]
fn the_built_program_matches_its_channel() {
    let bytes = std::fs::read(program()).expect("the built tsw-launch");
    let out = Command::new(program()).arg("--version").output().expect("run --version");
    let version = String::from_utf8_lossy(&out.stdout).into_owned();
    match env!("TSW_CHANNEL") {
        "stable" => {
            for needle in overlay_values() {
                assert!(!contains(&bytes, &needle), "a STABLE build must not contain {needle:?}");
            }
            assert!(!version.contains("testing"), "a stable build does not announce testing: {version}");
        }
        "testing" => {
            assert!(contains(&bytes, env!("TSW_CHANNEL_MARKER")), "a testing build carries its marker");
            assert!(contains(&bytes, env!("TSW_CHANNEL_APP_ID")));
            assert!(version.contains("testing build") && version.contains(env!("TSW_CHANNEL_MARKER")), "{version}");
        }
        other => panic!("unknown channel {other}"),
    }
}
