//! Central project constants (mirror of `packaging/project.conf`; a test keeps both identical).
//!
//! `PATCH_PUBLIC_KEY` is empty ON PURPOSE: the real minisign public key does not exist yet (the owner generates the key pair; the private key
//! goes into GitHub secrets, never into the repository). While it is empty the client patch system is disabled and the game starts as before.

/// Application version of the launcher (CLI, GUI and Flatpak): the crate version, which `scripts/version.py` keeps equal to
/// `APP_VERSION` in `packaging/project.conf` (a test checks it). Not the patch engine version and not the client patch level.
pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
/// One line for `--version` and the diagnostics: the three version axes side by side, so they cannot be mistaken for each other.
pub fn version_line(program: &str) -> String {
    format!("{program} {APP_VERSION} (patch engine tsw-patch {}, patch manifest format {})", tsw_patch::ENGINE_VERSION, tsw_patch::MANIFEST_FORMAT)
}

pub fn engine_version() -> &'static str {
    tsw_patch::ENGINE_VERSION
}
pub fn manifest_format() -> u32 {
    tsw_patch::MANIFEST_FORMAT
}

pub const PROJECT_NAME: &str = "TSW-Forever";
pub const REPO_URL: &str = "https://github.com/Glaubenskrieger/TSW-Forever";
pub const FLATPAK_REPO_URL: &str = "https://glaubenskrieger.github.io/TSW-Forever/repo";
pub const PATCH_BASE_URL: &str = "https://github.com/Glaubenskrieger/TSW-Forever/releases/latest/download/";
pub const APP_ID: &str = "org.tswlauncher.Launcher";
/// Base64 line of the minisign public key that signs patch manifests. Empty = patch system disabled.
pub const PATCH_PUBLIC_KEY: &str = "";
/// Flatpak ref of the launcher on the update source, for the given branch (channel: `stable` or `testing`).
pub fn flatpak_ref_for(branch: &str) -> String {
    format!("app/{APP_ID}/x86_64/{branch}")
}

/// The stable channel's ref (the default).
pub fn flatpak_ref() -> String {
    flatpak_ref_for("stable")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn conf(key: &str) -> String {
        include_str!("../../packaging/project.conf").lines().find_map(|l| l.strip_prefix(&format!("{key}="))).unwrap_or_else(|| panic!("{key} missing")).to_string()
    }

    #[test]
    fn the_constants_equal_the_central_configuration_file() {
        assert_eq!(conf("APP_VERSION"), APP_VERSION, "scripts/version.py set <version> keeps Cargo.toml and project.conf together");
        assert_eq!(conf("PATCH_MANIFEST_FORMAT"), tsw_patch::MANIFEST_FORMAT.to_string());
        assert_eq!(conf("PROJECT_NAME"), PROJECT_NAME);
        assert_eq!(conf("REPO_URL"), REPO_URL);
        assert_eq!(conf("FLATPAK_REPO_URL"), FLATPAK_REPO_URL);
        assert_eq!(conf("PATCH_BASE_URL"), PATCH_BASE_URL);
        assert_eq!(conf("APP_ID"), APP_ID);
        assert_eq!(format!("https://github.com/{}/{}", conf("GITHUB_OWNER"), conf("GITHUB_REPO")), REPO_URL);
    }

    #[test]
    fn the_version_line_names_the_three_axes_and_matches_the_crates() {
        let l = version_line("tsw-launch");
        assert!(l.starts_with(&format!("tsw-launch {APP_VERSION} (patch engine tsw-patch ")) && l.contains("manifest format 1"), "{l}");
        assert_ne!(APP_VERSION, tsw_patch::ENGINE_VERSION, "launcher and patch engine are versioned independently");
    }

    #[test]
    fn update_sources_are_https_and_patch_urls_end_in_a_slash() {
        for u in [REPO_URL, FLATPAK_REPO_URL, PATCH_BASE_URL] {
            assert!(u.starts_with("https://"), "{u}");
        }
        assert!(PATCH_BASE_URL.ends_with('/') && PATCH_BASE_URL.starts_with(REPO_URL));
        assert_eq!(flatpak_ref(), "app/org.tswlauncher.Launcher/x86_64/stable");
        assert_eq!(flatpak_ref_for("testing"), "app/org.tswlauncher.Launcher/x86_64/testing");
    }
}
