//! Build-time channel identity of the launcher (`stable` or `testing`).
//!
//! The channel is decided when the program is BUILT (`TSW_CHANNEL=testing cargo build`, default `stable`), never by a setting a user can edit.
//! * stable: `packaging/channels/stable.conf` and `APP_ID` of `packaging/project.conf`. Nothing about testing is compiled in.
//! * testing: needs `packaging/channels/testing.conf`, which exists only in the private development repository. Without it the build stops, so a testing
//!   program cannot be made from the public source.

use std::collections::HashMap;
use std::path::Path;

fn conf(path: &Path) -> HashMap<String, String> {
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    text.lines().filter(|l| !l.trim_start().starts_with('#')).filter_map(|l| l.split_once('=')).map(|(k, v)| (k.trim().to_string(), v.trim().to_string())).collect()
}

fn main() {
    println!("cargo:rerun-if-env-changed=TSW_CHANNEL");
    println!("cargo:rustc-check-cfg=cfg(channel_testing)");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("packaging");
    let channel = std::env::var("TSW_CHANNEL").unwrap_or_else(|_| "stable".into());
    let stable = conf(&root.join("channels").join("stable.conf"));
    let project = conf(&root.join("project.conf"));
    let (app_id, marker) = match channel.as_str() {
        "stable" => {
            assert_eq!(stable.get("CHANNEL").map(String::as_str), Some("stable"), "stable.conf must define CHANNEL=stable");
            (project.get("APP_ID").cloned().expect("APP_ID in packaging/project.conf"), String::new())
        }
        "testing" => {
            let path = root.join("channels").join("testing.conf");
            assert!(path.is_file(), "TSW_CHANNEL=testing needs {} (private development repository only)", path.display());
            let testing = conf(&path);
            assert_eq!(testing.get("CHANNEL").map(String::as_str), Some("testing"), "testing.conf must define CHANNEL=testing");
            println!("cargo:rustc-cfg=channel_testing");
            (testing.get("APP_ID").cloned().expect("APP_ID in testing.conf"), testing.get("MARKER").cloned().expect("MARKER in testing.conf"))
        }
        other => panic!("TSW_CHANNEL must be `stable` or `testing`, not `{other}`"),
    };
    let key = stable.get("UPDATE_PUBLIC_KEY").cloned().unwrap_or_default();
    assert!(key.len() < 200 && key.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '/' | '=')), "UPDATE_PUBLIC_KEY must be one base64 line or empty");
    println!("cargo:rustc-env=TSW_CHANNEL={channel}");
    println!("cargo:rustc-env=TSW_CHANNEL_APP_ID={app_id}");
    println!("cargo:rustc-env=TSW_UPDATE_KEY_STABLE={key}");
    println!("cargo:rustc-env=TSW_CHANNEL_MARKER={marker}");
}
