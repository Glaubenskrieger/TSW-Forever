//! The FINISHED launcher window program must not contain the experimental Windows self-update.
//!
//! Product decision (2026-11): Windows has no automatic update; newer versions are installed by hand from the official GitHub release. The signed-metadata updater is
//! kept for development behind the Cargo feature `windows-self-update`, which no product build enables. This test scans the built `tsw-launcher` program for strings
//! that exist only in that code (the metadata header, its field names, the installer command line, its error texts). Without the feature none may be present; with the
//! feature at least one must be (otherwise the list would silently stop proving anything).

use std::path::PathBuf;

const UPDATER_ONLY_STRINGS: &[&str] = &[
    "tsw-forever-update 1",
    "windows-sha256",
    "previous-installer",
    "/SUPPRESSMSGBOXES",
    "/CLOSEAPPLICATIONS",
    "the update metadata signature is not valid",
    "the downloaded file does not match the signed checksum",
];

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle.as_bytes())
}

#[test]
fn the_built_launcher_contains_the_self_update_only_when_the_development_feature_is_on() {
    let bytes = std::fs::read(PathBuf::from(env!("CARGO_BIN_EXE_tsw-launcher"))).expect("the built tsw-launcher");
    let present: Vec<&str> = UPDATER_ONLY_STRINGS.iter().copied().filter(|n| contains(&bytes, n)).collect();
    if cfg!(feature = "windows-self-update") {
        assert!(!present.is_empty(), "with the development feature the scan must find the updater (the scan list is stale otherwise)");
    } else {
        assert!(present.is_empty(), "a product build must not contain the Windows self-update, found: {present:?}");
    }
}

#[test]
fn the_notice_is_in_the_windows_build_and_is_no_updater() {
    let bytes = std::fs::read(PathBuf::from(env!("CARGO_BIN_EXE_tsw-launcher"))).expect("the built tsw-launcher");
    // The newer-version notice exists on Windows only (its call site is `#[cfg(windows)]`, so a Linux program does not contain the release-page address: the linker drops
    // the code). Only this assertion is platform-specific; the updater check above applies everywhere.
    // On Windows the notice asks the official release page and opens it (the absence of any download or start of a downloaded file is shown by the first test and by the
    // unit tests of `releasenotice`: it parses only a redirect header and builds the page address itself).
    if cfg!(windows) {
        assert!(contains(&bytes, "/releases/latest"), "the Windows notice asks the latest release");
        assert!(contains(&bytes, "/releases/tag/v"), "and opens the official release page of that version");
    }
    // other platforms: nothing of the Windows notice is required (the Linux update path is the Flatpak portal)
}
