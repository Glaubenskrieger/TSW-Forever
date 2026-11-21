//! Guard for the launcher update (owner decision 2026-11-21, version 0.4.1): on Linux and on Windows the launcher NEVER installs an update by itself. It only shows a notice that a
//! newer stable version exists, and the notice's button opens the download area of the official web site. These are source-level checks, so that a later change which brings a second
//! mechanism back (a portal client, a downloader, an installer start, a hosted Flatpak repository) fails here first.

const MAIN: &str = include_str!("../src/main.rs");
const NOTICE: &str = include_str!("../../launcher/src/releasenotice.rs");
const UPDATE_MOD: &str = include_str!("../../launcher/src/update/mod.rs");
const CARGO: &str = include_str!("../Cargo.toml");

fn non_test_part(src: &str) -> &str {
    src.split("#[cfg(test)]").next().unwrap()
}

/// The window program (its inline test doubles contain none of the strings checked below).
fn product_main() -> String {
    MAIN.to_string()
}

#[test]
fn the_flatpak_update_portal_and_the_hosted_repository_are_gone() {
    let main = product_main();
    for gone in ["flatpak_update", "UpdateMonitor", "CreateUpdateMonitor", "update::flatpak", "selfupdate", "flatpak_ref", "FLATPAK_REPO_URL", "refs/heads/"] {
        assert!(!main.contains(gone), "main.rs must not contain {gone}");
        assert!(!non_test_part(NOTICE).contains(gone), "releasenotice.rs must not contain {gone}");
    }
    assert!(!UPDATE_MOD.contains("pub mod flatpak"), "no Flatpak update module");
    assert!(!std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/src/flatpak_update.rs")).exists(), "the portal client file is gone");
    assert!(!std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../launcher/src/selfupdate.rs")).exists(), "the plain-hint fallback file is gone");
}

#[test]
fn the_notice_asks_only_the_published_release_metadata_and_never_starts_anything_but_curl() {
    let code = non_test_part(NOTICE);
    assert_eq!(code.matches("Command::new(").count(), 1, "the notice may start exactly one program: curl, to read the redirect of the latest release");
    for forbidden in ["\"flatpak\"", "\"install\"", "std::fs::write", "File::create", "remove_file", "rename(", "explorer", "xdg-open"] {
        assert!(!code.contains(forbidden), "releasenotice.rs must not contain {forbidden}: it only detects");
    }
    assert!(code.contains("\"=https\"") && code.contains("\"--max-redirs\"") && code.contains("\"--head\""), "https only, no redirect followed, headers only");
}

#[test]
fn the_window_only_opens_the_download_area_and_has_no_installation_path() {
    let main = product_main();
    // the one place that starts a program for the notice: the browser opener, and only for the two addresses of the web site's download areas
    let open = main.find("fn open_download_page(").expect("the opener exists");
    let body = &main[open..open + main[open..].find("\n}\n").unwrap()];
    assert!(body.contains("allowed.iter().any(|a| a == url)"), "the opener checks the address first");
    assert!(body.contains("explorer.exe") && body.contains("xdg-open"));
    // nothing in the window program downloads or starts an installer
    for forbidden in ["flatpak install", "flatpak update", "\"--user\"", "setup.exe", "/VERYSILENT", "install_launcher_update", "request_launcher_update", "request_update"] {
        assert!(!main.contains(forbidden), "main.rs must not contain {forbidden}");
    }
    // the notice's button is the only action behind the update callback in a product build
    let action = main.find("ui.on_update_action(").expect("the callback is wired");
    let handler = &main[action..action + 1800];
    assert!(handler.contains("open_download_page(&notice.url)"), "{handler}");
}

#[test]
fn the_notice_check_is_started_on_both_platforms_and_the_updater_feature_stays_off() {
    let main = product_main();
    let start = main.find("start_release_notice_check(&ui, &state);").expect("started at launch");
    assert!(!main[start.saturating_sub(120)..start].contains("#[cfg(windows)]"), "not Windows-only any more: Linux asks as well");
    assert!(CARGO.contains("default = []"), "the development updater feature is not part of the default build");
}

#[test]
fn only_a_release_build_asks_on_linux_and_the_branch_name_decides_nothing() {
    // build metadata (not the Flatpak branch, which is `local` for releases and developer builds alike) lets the launcher ask for a newer version
    assert!(MAIN.contains("tsw_launch::channel::RELEASE_BUILD"), "the notice check passes the release-build metadata to the eligibility check");
    assert!(NOTICE.contains("release_build && build == Channel::Stable"), "a development build never asks");
    assert!(!non_test_part(NOTICE).contains("branch=stable"), "no branch name is required");
}
